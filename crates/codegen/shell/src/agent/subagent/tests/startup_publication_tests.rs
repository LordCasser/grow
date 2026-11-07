use super::*;

fn spawned() -> SessionUpdate {
    SessionUpdate::SubagentSpawned {
        subagent_id: "child".into(),
        parent_session_id: "parent".into(),
        parent_prompt_id: None,
        child_session_id: "child".into(),
        subagent_type: "explore".into(),
        description: "publication test".into(),
        effective_context_source: None,
        context_normalized: false,
        capability_mode: None,
        permission_mode: None,
        effective_permission_mode: None,
        model: None,
        reasoning_effort: None,
        model_state: None,
        workflow_agent_names: None,
        resumed_from: None,
        workflow_run_id: None,
        goal_id: None,
    }
}

#[tokio::test]
async fn startup_publication_closed_parent_never_bypasses_its_owner() {
    let (gateway, mut gateway_rx) = test_gateway_with_receiver();
    let (parent, commands) = mpsc::unbounded_channel();
    drop(commands);
    let error = publish_subagent_spawned(&gateway, "parent", spawned(), Some(&parent))
        .await
        .unwrap_err();
    assert!(error.contains("parent actor mailbox closed"));
    assert!(gateway_rx.try_recv().is_err());
}

#[tokio::test]
async fn startup_publication_direct_fallback_requires_handler_completion() {
    for acknowledge in [true, false] {
        let (gateway, mut gateway_rx) = test_gateway_with_receiver();
        let publication = publish_subagent_spawned(&gateway, "parent", spawned(), None);
        tokio::pin!(publication);
        assert!(futures::poll!(&mut publication).is_pending());
        let acp_transport::AcpClientMessage::ExtNotification(message) =
            gateway_rx.recv().await.unwrap()
        else {
            panic!("expected original Spawned gateway notification");
        };
        let body: serde_json::Value = serde_json::from_str(message.request.params.get()).unwrap();
        assert!(
            body["_meta"]["eventId"]
                .as_str()
                .unwrap()
                .starts_with("parent-")
        );
        assert!(futures::poll!(&mut publication).is_pending());
        if acknowledge {
            message.response_tx.send(Ok(())).unwrap();
            publication.await.unwrap();
        } else {
            drop(message.response_tx);
            assert!(
                publication
                    .await
                    .unwrap_err()
                    .contains("completion dropped")
            );
        }
        assert!(gateway_rx.try_recv().is_err(), "never retry Spawned");
    }
}

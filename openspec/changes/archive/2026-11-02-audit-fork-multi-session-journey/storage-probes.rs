// Audit-only probe. Append inside session::storage::jsonl::tests.
#[tokio::test]
async fn audit_fork_journey_delete_independent_fork() {
    let temp = TempDir::new().unwrap();
    let adapter = JsonlStorageAdapter::with_root(temp.path().to_path_buf());
    let source = create_test_info();
    adapter.init_session(&source, default_model_id()).await.unwrap();
    append_timeline_seed(&adapter, &source, vec![ConversationItem::user("audit history")]).await;
    let fork = Info { id: acp::SessionId::new("audit-independent-fork"), cwd: source.cwd.clone() };
    adapter.copy_session_data_sync(&source, &fork, CopySessionOptions {
        parent_session_id: Some(source.id.to_string()), ..Default::default()
    }).unwrap();
    let events = adapter.open_session(&fork).unwrap().validated_timeline(fork.id.0.as_ref()).unwrap().timeline;
    assert!(!events.events().iter().any(|e| matches!(e.kind,
        chat_state::TimelineEventKind::Subagent(_) | chat_state::TimelineEventKind::SubagentSeed(_))));
    adapter.delete_session(&fork).await.expect("a standalone fork with only inherited history has no child result dependency");
    assert!(adapter.session_dir(&source).is_dir());
}

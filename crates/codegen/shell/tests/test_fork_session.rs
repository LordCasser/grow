//! Integration tests for the fork session flow.
//!
//! These tests verify the complete fork session flow:
//! 1. Fork session data with parent tracking
//! 2. Verify forked session has correct metadata
//! 3. Test worktree creation from worktree types

use acp_transport::protocol as acp;
use shell::sampling::ConversationItem;
use shell::session::info::Info;
use shell::session::storage::{JsonlStorageAdapter, StorageAdapter};
use tempfile::TempDir;

/// Helper to create a test session in a temp directory
async fn create_test_session(storage: &JsonlStorageAdapter, session_id: &str, cwd: &str) -> Info {
    let info = Info {
        id: acp::SessionId::new(session_id),
        cwd: cwd.to_string(),
    };

    let model_id = shell::agent::models::ModelId::new("grow-code-fast-1");
    storage.init_session(&info, model_id).await.unwrap();

    // Seed the canonical Timeline.
    let msg = ConversationItem::user("Hello world");
    let timeline = chat_state::Timeline::from_seed(vec![msg]).unwrap();
    for event in timeline.events() {
        storage.append_timeline_event(&info, event).await.unwrap();
    }

    // Add an update
    let notification = acp::SessionNotification::new(
        acp::SessionId::new(session_id),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new("Test response".to_string()),
        ))),
    );
    storage
        .append_update(
            &info,
            &shell::session::storage::SessionUpdate::Acp(Box::new(notification)),
        )
        .await
        .unwrap();

    info
}

#[tokio::test]
async fn test_fork_session_creates_new_session_with_parent_tracking() {
    let temp_dir = TempDir::new().unwrap();
    let storage = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());

    // Create source session
    let source_info = create_test_session(&storage, "source-session-123", "/source/path").await;

    let target_info = Info {
        id: acp::SessionId::new("fork-session-456"),
        cwd: "/new/path".to_string(),
    };

    let options = shell::session::storage::CopySessionOptions {
        parent_session_id: Some("source-session-123".to_string()),
        new_model_id: Some("grow-3".to_string()),
        target_prompt_index: None,
        ..Default::default()
    };

    let result = storage
        .copy_session_data(&source_info, &target_info, options)
        .await
        .unwrap();

    // Verify result
    assert_eq!(result.surface_items_copied, 1);
    assert_eq!(result.updates_copied, 1);

    // Load the forked session and verify metadata
    let loaded = storage.load_session(&target_info).await.unwrap();

    assert_eq!(loaded.summary.info.id.to_string(), "fork-session-456");
    assert_eq!(loaded.summary.info.cwd, "/new/path");
    assert_eq!(
        loaded.summary.current_model_id,
        shell::agent::models::ModelId::new("grow-3")
    );
    assert_eq!(
        loaded.summary.parent_session_id,
        Some("source-session-123".to_string())
    );
    assert!(loaded.summary.forked_at.is_some());

    // Verify the canonical Timeline projects the copied Surface.
    let timeline = chat_state::Timeline::from_events(loaded.timeline_events.clone()).unwrap();
    assert_eq!(timeline.surface().len(), 1);

    // Verify updates were copied with transformed session ID
    assert_eq!(loaded.updates.len(), 1);
    match &loaded.updates[0] {
        shell::session::storage::SessionUpdate::Acp(notification) => {
            assert_eq!(notification.session_id.to_string(), "fork-session-456");
        }
        _ => panic!("Expected ACP update"),
    }
}

#[tokio::test]
async fn test_fork_starts_new_title_lineage() {
    let temp_dir = TempDir::new().unwrap();
    let storage = JsonlStorageAdapter::with_root(temp_dir.path().to_path_buf());

    // Create source session
    let source_info = create_test_session(&storage, "titled-session", "/source").await;

    // Commit the source title as a canonical Timeline fact.
    let mut source_timeline =
        chat_state::Timeline::from_seed(vec![ConversationItem::user("Hello world")]).unwrap();
    let title_event = source_timeline
        .record(chat_state::TimelineEventKind::SessionTitle(
            chat_state::SessionTitleEvent {
                title: "My Important Session".to_string(),
                source: chat_state::SessionTitleSource::User,
            },
        ))
        .unwrap();
    storage
        .append_timeline_event_durable(&source_info, &title_event)
        .await
        .unwrap();

    // Fork the session
    let target_info = Info {
        id: acp::SessionId::new("fork-titled"),
        cwd: "/new".to_string(),
    };

    let options = shell::session::storage::CopySessionOptions {
        parent_session_id: Some("titled-session".to_string()),
        new_model_id: None,
        target_prompt_index: None,
        ..Default::default()
    };

    storage
        .copy_session_data(&source_info, &target_info, options)
        .await
        .unwrap();

    // A fork inherits Surface, not parent metadata identity. It generates or
    // receives a title in its own Timeline lineage.
    let loaded = storage.load_session(&target_info).await.unwrap();
    assert_eq!(loaded.summary.display_title(), "");
}

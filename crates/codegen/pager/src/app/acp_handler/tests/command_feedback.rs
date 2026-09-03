use super::*;
use shell::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};

fn notice(id: &str, tone: UiNoticeTone, message: &str) -> GrowSessionUpdate {
    GrowSessionUpdate::UiNotice(UiNotice {
        correlation_id: id.into(),
        category: UiNoticeCategory::Command,
        subject: Some("/goal edit ship it".into()),
        description: Some("Manage a Goal".into()),
        message: message.into(),
        tone,
        details: None,
    })
}

#[test]
fn command_progress_is_live_and_does_not_erase_newer_progress() {
    let mut app = make_app_with_agent("s1");
    for (id, message) in [("old", "queued"), ("new", "running")] {
        assert!(handle(
            make_ext_session_notification("s1", notice(id, UiNoticeTone::Progress, message)),
            &mut app
        ));
    }
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 0);
    assert!(handle(
        make_ext_session_notification("s1", notice("old", UiNoticeTone::Success, "Updated")),
        &mut app
    ));
    assert_eq!(
        app.agents[&AgentId(0)].session.live_status(160).as_deref(),
        Some("running")
    );
    assert!(handle(
        make_ext_session_notification(
            "s1",
            notice(
                "new",
                UiNoticeTone::Error,
                "Rejected: Goal was cleared. Create a new Goal."
            )
        ),
        &mut app
    ));
    let agent = &app.agents[&AgentId(0)];
    assert!(agent.session.live_status(160).is_none());
    assert_eq!(agent.scrollback.len(), 2);
    assert!(!handle(
        make_ext_session_notification("s1", notice("new", UiNoticeTone::Progress, "late queued")),
        &mut app
    ));
    assert!(app.agents[&AgentId(0)].session.live_status(160).is_none());
}

#[test]
fn replay_does_not_reanimate_command_progress_and_keeps_one_terminal_event() {
    let mut app = make_app_with_agent("s1");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .begin_replay();
    assert!(!handle(
        make_replayed_ext_session_notification(
            "s1",
            "progress",
            notice("invoke", UiNoticeTone::Progress, "queued")
        ),
        &mut app
    ));
    let terminal = notice("invoke", UiNoticeTone::Success, "Updated");
    handle(
        make_replayed_ext_session_notification("s1", "terminal", terminal.clone()),
        &mut app,
    );
    handle(
        make_replayed_ext_session_notification("s1", "terminal", terminal),
        &mut app,
    );
    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 1);
    assert!(agent.session.live_status(160).is_none());
}

#[test]
fn memory_browser_requires_a_matching_live_query_and_opens_only_once() {
    let mut app = make_app_with_agent("s1");
    let result = |id: &str| GrowSessionUpdate::MemoryFiles {
        invocation_id: id.into(),
        files: vec![],
    };
    assert!(!handle(
        make_ext_session_notification("s1", result("unsolicited")),
        &mut app
    ));
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .pending_memory_browse = Some("latest".into());
    assert!(!handle(
        make_ext_session_notification("s1", result("stale")),
        &mut app
    ));
    assert!(handle(
        make_ext_session_notification("s1", result("latest")),
        &mut app
    ));
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    assert!(matches!(
        agent.active_modal,
        Some(crate::views::modal::ActiveModal::MemoryBrowser { .. })
    ));
    agent.active_modal = None;
    assert!(!handle(
        make_ext_session_notification("s1", result("latest")),
        &mut app
    ));
    assert!(app.agents[&AgentId(0)].active_modal.is_none());
}

#[test]
fn memory_failure_or_reconnect_cannot_later_open_an_empty_browser() {
    let mut app = make_app_with_agent("s1");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .pending_memory_browse = Some("memory".into());
    handle(
        make_ext_session_notification(
            "s1",
            notice(
                "memory",
                UiNoticeTone::Warning,
                "Memory disabled. Use /memory on.",
            ),
        ),
        &mut app,
    );
    assert!(!handle(
        make_ext_session_notification(
            "s1",
            GrowSessionUpdate::MemoryFiles {
                invocation_id: "memory".into(),
                files: vec![]
            }
        ),
        &mut app
    ));
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.pending_memory_browse = Some("next".into());
    agent.session.begin_replay();
    assert!(agent.session.pending_memory_browse.is_none());
    assert!(!handle(
        make_replayed_ext_session_notification(
            "s1",
            "old-memory-result",
            GrowSessionUpdate::MemoryFiles {
                invocation_id: "next".into(),
                files: vec![]
            }
        ),
        &mut app
    ));
    assert!(app.agents[&AgentId(0)].active_modal.is_none());
}

#[test]
fn manual_compaction_is_immediate_and_never_flushes_again_in_a_later_turn() {
    let mut app = make_app_with_agent("s1");
    let update = GrowSessionUpdate::AutoCompactCompleted {
        manual: true,
        async_compact: false,
        tokens_before: 90_000,
        tokens_after: 20_000,
        elapsed_ms: Some(500),
        summary_preview: None,
    };
    handle(make_ext_session_notification("s1", update), &mut app);
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::CompactionCompleted {
            tokens_after: 20_000,
            ..
        })
    ));
    let before = agent.scrollback.len();
    agent.session.note_context_used(21_000);
    agent.session.finish_turn(&mut agent.scrollback);
    assert_eq!(agent.scrollback.len(), before);
}

#[test]
fn compaction_terminal_replay_preserves_one_immutable_result() {
    for update in [
        GrowSessionUpdate::AutoCompactCompleted {
            manual: true,
            async_compact: false,
            tokens_before: 90_000,
            tokens_after: 20_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        },
        GrowSessionUpdate::AutoCompactCompleted {
            manual: false,
            async_compact: true,
            tokens_before: 90_000,
            tokens_after: 20_000,
            elapsed_ms: Some(500),
            summary_preview: None,
        },
        GrowSessionUpdate::AutoCompactFailed {
            error: "provider unavailable".into(),
        },
        GrowSessionUpdate::AutoCompactCancelled {
            reason: shell::extensions::notification::AutoCompactCancelReason::UserCancelled,
        },
    ] {
        let mut app = make_app_with_agent("s1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .begin_replay();
        for _ in 0..2 {
            handle(
                make_replayed_ext_session_notification("s1", "compact-terminal", update.clone()),
                &mut app,
            );
        }
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 1);
    }
}

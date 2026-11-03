use super::*;

fn finish_child(app: &mut AppView, sid: &str) {
    handle(
        make_ext_session_notification("sess-parent", test_subagent_finished(sid)),
        app,
    );
}

fn assert_task_and_tool(parent: &AgentView, sid: &str, task: &str) {
    let child = &parent.subagent_views[sid];
    assert_eq!(child_scrollback_tool_call_count(parent, sid), 1);
    assert_eq!(
        child
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| {
                matches!(&entry.block, RenderBlock::UserPrompt(prompt) if prompt.text == task)
            })
            .count(),
        1,
    );
    assert_eq!(
        child
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| {
                matches!(&entry.block, RenderBlock::SessionEvent(event)
                if matches!(event.event, SessionEvent::TurnCompleted { .. }))
            })
            .count(),
        1,
    );
}

fn leave_child(parent: &mut AgentView) {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    parent.handle_input(
        &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        &crate::actions::ActionRegistry::defaults(),
        &mut Vec::new(),
    );
    assert!(parent.active_subagent.is_none());
}

#[test]
fn completed_subagent_transcripts_release_and_restore_across_batches() {
    with_replay_disk_home(|home| {
        let mut app = make_app_with_agent("sess-parent");
        let task = "task payload ".repeat(4096);
        for index in 0..16 {
            let sid = format!("reclaimed-child-{index}");
            let history = format!(
                "{}\n{}",
                child_user_message_line(&sid, &task),
                child_tool_line(&sid)
            );
            spawn_subagent_with_optional_history(home, &mut app, &sid, Some(&history));
            app.reclaim_finished_subagent_transcripts();
            assert_eq!(
                child_scrollback_tool_call_count(&app.agents[&AgentId(0)], &sid),
                1
            );
            finish_child(&mut app, &sid);
            app.reclaim_finished_subagent_transcripts();
            let parent = &app.agents[&AgentId(0)];
            assert_eq!(parent.session.subagent_sessions.len(), index + 1);
            assert!(
                parent
                    .subagent_views
                    .values()
                    .all(|child| child.scrollback.is_empty())
            );
        }
        for index in 0..16 {
            let sid = format!("reclaimed-child-{index}");
            let parent = app.agents.get_mut(&AgentId(0)).unwrap();
            parent.open_subagent_fullscreen(sid.clone());
            assert_task_and_tool(parent, &sid, &task);
            let len = parent.subagent_views[&sid].scrollback.len();
            parent.open_subagent_fullscreen(sid.clone());
            assert_eq!(parent.subagent_views[&sid].scrollback.len(), len);
            leave_child(parent);
            app.reclaim_finished_subagent_transcripts();
            assert!(
                app.agents[&AgentId(0)].subagent_views[&sid]
                    .scrollback
                    .is_empty()
            );
        }
    });
}

#[test]
fn completed_subagent_reclamation_preserves_selected_view_draft_controls_and_highwaters() {
    with_replay_disk_home(|home| {
        let sid = "selected-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        let history = format!(
            "{}\n{}",
            child_user_message_line(sid, "original task"),
            child_tool_line(sid)
        );
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&history));
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(sid.into());
        finish_child(&mut app, sid);
        let control = {
            let child = app
                .agents
                .get_mut(&AgentId(0))
                .unwrap()
                .subagent_views
                .get_mut(sid)
                .unwrap();
            child.prompt.set_text("keep unsent draft");
            child.session.begin_model_switch_for_test();
            child.session.last_applied_event_seq = Some(100_000);
            child.session.last_applied_grow_event_seq = Some(200_000);
            child
                .inline_media_cache
                .insert(PathBuf::from("/tmp/reclaim-image"), vec![0; 65536]);
            child.session.current_control_token_for_test()
        };
        app.reclaim_finished_subagent_transcripts();
        assert_task_and_tool(&app.agents[&AgentId(0)], sid, "original task");
        leave_child(app.agents.get_mut(&AgentId(0)).unwrap());
        app.reclaim_finished_subagent_transcripts();
        let child = &app.agents[&AgentId(0)].subagent_views[sid];
        assert!(child.scrollback.is_empty());
        assert!(child.inline_media_cache.is_empty());
        assert_eq!(child.prompt.text(), "keep unsent draft");
        assert_eq!(child.session.current_control_token_for_test(), control);
        assert_eq!(child.session.last_applied_event_seq, Some(100_000));
        assert_eq!(child.session.last_applied_grow_event_seq, Some(200_000));
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.open_subagent_fullscreen(sid.into());
        assert_task_and_tool(parent, sid, "original task");
        assert_eq!(
            parent.subagent_views[sid]
                .session
                .current_control_token_for_test(),
            control
        );
    });
}

#[test]
fn completed_subagent_reclamation_keeps_replay_and_response_owners() {
    with_replay_disk_home(|home| {
        let sid = "owned-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&child_tool_line(sid)));
        finish_child(&mut app, sid);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;
        app.reclaim_finished_subagent_transcripts();
        assert_eq!(
            child_scrollback_tool_call_count(&app.agents[&AgentId(0)], sid),
            1
        );
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = false;
        let (permission, mut response) = make_permission_message(sid);
        handle(permission, &mut app);
        app.reclaim_finished_subagent_transcripts();
        let parent = &app.agents[&AgentId(0)];
        assert_eq!(parent.permission_queue.len(), 1);
        assert!(matches!(
            response.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(parent.subagent_views.contains_key(sid));
    });
}

#[test]
fn completed_subagent_history_failure_remains_retryable_without_live_tail() {
    with_replay_disk_home(|home| {
        let sid = "failed-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        let history = format!(
            "{}\n{}",
            child_user_message_line(sid, "recover this task"),
            child_tool_line(sid)
        );
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&history));
        finish_child(&mut app, sid);
        app.reclaim_finished_subagent_transcripts();
        let path = home
            .join("sessions/%2Ftmp")
            .join(sid)
            .join("timeline.jsonl");
        let original = std::fs::read(&path).unwrap();
        std::fs::write(&path, b"invalid journal\n").unwrap();
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(sid.into());
        for _ in 0..3 {
            handle(
                make_agent_chunk_message(sid, "must not block recovery"),
                &mut app,
            );
        }
        let parent = &app.agents[&AgentId(0)];
        assert!(!parent.session.subagent_sessions[sid].child_updates_replayed);
        assert_eq!(
            parent.subagent_views[sid].scrollback.len(),
            1,
            "one bounded error notice, no partial history/live tail"
        );
        assert!(
            matches!(&parent.subagent_views[sid].scrollback.entry(0).unwrap().block,
            RenderBlock::Notice(notice) if notice.text.contains("Could not load child history"))
        );
        std::fs::write(path, original).unwrap();
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        parent.open_subagent_fullscreen(sid.into());
        assert_task_and_tool(parent, sid, "recover this task");
        assert!(parent.session.subagent_sessions[sid].child_updates_replayed);
        assert!(!parent.subagent_views[sid].scrollback.iter_entries().any(|(_, entry)| {
            matches!(&entry.block, RenderBlock::Notice(notice) if notice.text.contains("child history"))
        }));
    });
}

#[test]
fn completed_subagent_missing_history_is_retryable_after_source_returns() {
    with_replay_disk_home(|home| {
        let sid = "missing-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        let history = format!(
            "{}\n{}",
            child_user_message_line(sid, "restore missing task"),
            child_tool_line(sid)
        );
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&history));
        finish_child(&mut app, sid);
        app.reclaim_finished_subagent_transcripts();
        let directory = home.join("sessions/%2Ftmp").join(sid);
        let held = home.join("held-child");
        std::fs::rename(&directory, &held).unwrap();
        let parent = app.agents.get_mut(&AgentId(0)).unwrap();
        for _ in 0..3 {
            parent.open_subagent_fullscreen(sid.into());
            assert!(!parent.session.subagent_sessions[sid].child_updates_replayed);
            assert_eq!(parent.subagent_views[sid].scrollback.len(), 1);
        }
        std::fs::rename(held, directory).unwrap();
        parent.open_subagent_fullscreen(sid.into());
        assert_task_and_tool(parent, sid, "restore missing task");
        assert!(!parent.subagent_views[sid].scrollback.iter_entries().any(|(_, entry)| {
            matches!(&entry.block, RenderBlock::Notice(notice) if notice.text.contains("history is unavailable"))
        }));
    });
}

#[test]
fn completed_subagent_reclamation_waits_for_child_replay_and_viewer() {
    with_replay_disk_home(|home| {
        let sid = "viewer-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&child_tool_line(sid)));
        finish_child(&mut app, sid);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .get_mut(sid)
            .unwrap()
            .session
            .loading_replay = true;
        app.reclaim_finished_subagent_transcripts();
        assert_eq!(
            child_scrollback_tool_call_count(&app.agents[&AgentId(0)], sid),
            1
        );
        let viewer_path = home.join("viewer.txt");
        std::fs::write(&viewer_path, "owned file viewer").unwrap();
        let child = app
            .agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .get_mut(sid)
            .unwrap();
        child.session.loading_replay = false;
        child.line_viewer =
            crate::views::file_search::line_viewer::LineViewerState::open(&viewer_path, None);
        assert!(child.line_viewer.is_some());
        app.reclaim_finished_subagent_transcripts();
        assert_eq!(
            child_scrollback_tool_call_count(&app.agents[&AgentId(0)], sid),
            1
        );
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .get_mut(sid)
            .unwrap()
            .line_viewer = None;
        app.reclaim_finished_subagent_transcripts();
        assert!(
            app.agents[&AgentId(0)].subagent_views[sid]
                .scrollback
                .is_empty()
        );
    });
}

#[test]
fn completed_subagent_duplicate_does_not_rehydrate_unloaded_history() {
    with_replay_disk_home(|home| {
        let sid = "duplicate-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&child_tool_line(sid)));
        finish_child(&mut app, sid);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .get_mut(sid)
            .unwrap()
            .session
            .last_applied_event_seq = Some(100_000);
        app.reclaim_finished_subagent_transcripts();
        let mut duplicate = make_agent_chunk_message(sid, "duplicate output");
        if let AcpClientMessage::SessionNotification(notif) = &mut duplicate {
            notif.request.meta = serde_json::json!({"eventId": "duplicate-source-99999"})
                .as_object()
                .cloned();
        }
        handle(duplicate, &mut app);
        let parent = &app.agents[&AgentId(0)];
        assert!(parent.subagent_views[sid].scrollback.is_empty());
        assert!(!parent.session.subagent_sessions[sid].child_updates_replayed);
    });
}

#[test]
fn completed_subagent_late_finish_after_reclamation_has_one_footer() {
    with_replay_disk_home(|home| {
        let sid = "late-finish-reclaimed-child";
        let mut app = make_app_with_agent("sess-parent");
        let history = format!(
            "{}\n{}",
            child_user_message_line(sid, "finish once"),
            child_tool_line(sid)
        );
        spawn_subagent_with_optional_history(home, &mut app, sid, Some(&history));
        finish_child(&mut app, sid);
        app.reclaim_finished_subagent_transcripts();
        for _ in 0..3 {
            finish_child(&mut app, sid);
            assert_task_and_tool(&app.agents[&AgentId(0)], sid, "finish once");
        }
    });
}

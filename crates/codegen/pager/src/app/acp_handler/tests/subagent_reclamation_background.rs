use super::*;

use crate::scrollback::blocks::{BgTaskBlock, BgTaskKind};
use shell::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use tools::types::TaskSnapshot;

fn task_completed_notification(session_id: &str, task_id: &str) -> acp::ExtNotification {
    let now = std::time::SystemTime::now();
    let notif = GrowNotification {
        session_id: acp::SessionId::new(session_id),
        update: GrowUpdate::TaskCompleted {
            task_snapshot: TaskSnapshot {
                task_id: task_id.into(),
                command: "printf 'persisted output'".into(),
                display_command: None,
                cwd: "/tmp".into(),
                start_time: now,
                end_time: Some(now),
                output: String::new(),
                output_file: "/tmp/bg-task-output.log".into(),
                truncated: false,
                exit_code: Some(0),
                signal: None,
                completed: true,
                kind: Default::default(),
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                goal_id: None,
                goal_definition_revision: None,
                description: None,
                is_backgrounded: true,
            },
        },
        meta: None,
    };
    let raw = serde_json::value::to_raw_value(&notif).unwrap();
    acp::ExtNotification::new("grow/task_completed", std::sync::Arc::from(raw))
}

fn grow_history_line(session_id: &str, update: GrowUpdate) -> String {
    let notif = GrowNotification {
        session_id: acp::SessionId::new(session_id),
        update,
        meta: None,
    };
    serde_json::json!({
        "method": "_grow/session/update",
        "params": notif,
    })
    .to_string()
}

fn assert_task_lifecycle(parent: &AgentView, child_sid: &str, prompt: &str, output: &str) {
    let child = &parent.subagent_views[child_sid];
    let prompt_count = child
        .scrollback
        .iter_entries()
        .filter(|(_, entry)| {
            matches!(&entry.block, RenderBlock::UserPrompt(block) if block.text == prompt)
        })
        .count();
    assert_eq!(
        prompt_count, 1,
        "the original child prompt should render once"
    );

    let task_rows = child
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| match &entry.block {
            RenderBlock::BgTask(block) if block.task_id == "child-bg-task" => {
                Some((id, block.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(task_rows.len(), 2, "one started row and one terminal row");
    assert!(matches!(&task_rows[0].1.kind, BgTaskKind::Started));
    assert!(matches!(&task_rows[1].1.kind, BgTaskKind::Completed { .. }));

    let task = child
        .session
        .bg_tasks
        .get("child-bg-task")
        .expect("replayed background task state");
    assert_eq!(task.status, BgTaskStatus::Done);
    assert_eq!(task.stdout, output);
    assert_eq!(task.stdout_line_count, output.lines().count());
    assert_eq!(task.tool_call_id, "child-bg-tool-call");
    assert_eq!(
        child.session.bg_tool_call_to_task.get("child-bg-tool-call"),
        Some(&"child-bg-task".to_owned()),
        "stdout correlation must retain the child task owner"
    );
    let started_id = task.scrollback_entry_id.expect("started row owner");
    assert!(
        child.scrollback.get_by_id(started_id).is_some_and(|entry| {
            matches!(&entry.block, RenderBlock::BgTask(BgTaskBlock { task_id, kind: BgTaskKind::Started, .. }) if task_id == "child-bg-task")
        }),
        "the state owner must point to the surviving started row"
    );
}

fn assert_running_task(parent: &AgentView, child_sid: &str, prompt: &str, output: &str) {
    let child = &parent.subagent_views[child_sid];
    assert_eq!(
        child
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| {
                matches!(&entry.block, RenderBlock::UserPrompt(block) if block.text == prompt)
            })
            .count(),
        1,
        "the original child prompt should render once"
    );
    let task_rows = child
        .scrollback
        .iter_entries()
        .filter(|(_, entry)| {
            matches!(&entry.block, RenderBlock::BgTask(block) if block.task_id == "child-bg-task")
        })
        .count();
    assert_eq!(task_rows, 1, "live start should create one started row");
    let task = &child.session.bg_tasks["child-bg-task"];
    assert_eq!(task.status, BgTaskStatus::Running);
    assert_eq!(task.stdout, output);
    assert_eq!(task.tool_call_id, "child-bg-tool-call");
}

fn close_subagent_fullscreen(parent: &mut AgentView) {
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    parent.handle_input(
        &Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        &crate::actions::ActionRegistry::defaults(),
        &mut Vec::new(),
    );
    assert!(parent.active_subagent.is_none());
}

#[test]
fn finished_child_reclamation_restores_background_task_lifecycle_and_owner() {
    with_replay_disk_home(|home| {
        let child_sid = "reclaimed-background-child";
        let prompt = "run and preserve the background task";
        let output = "first output\nsecond output";
        let mut app = make_app_with_agent("sess-parent");
        let backgrounded = GrowUpdate::TaskBackgrounded {
            tool_call_id: "child-bg-tool-call".into(),
            task_id: "child-bg-task".into(),
            command: "printf 'persisted output'".into(),
            cwd: "/tmp".into(),
            output_file: "/tmp/bg-task-output.log".into(),
            monitor_description: None,
            description: None,
        };
        let completed = GrowUpdate::TaskCompleted {
            task_snapshot: TaskSnapshot {
                task_id: "child-bg-task".into(),
                command: "printf 'persisted output'".into(),
                display_command: None,
                cwd: "/tmp".into(),
                start_time: std::time::SystemTime::now(),
                end_time: Some(std::time::SystemTime::now()),
                output: String::new(),
                output_file: "/tmp/bg-task-output.log".into(),
                truncated: false,
                exit_code: Some(0),
                signal: None,
                completed: true,
                kind: Default::default(),
                block_waited: false,
                explicitly_killed: false,
                owner_session_id: None,
                goal_id: None,
                goal_definition_revision: None,
                description: None,
                is_backgrounded: true,
            },
        };
        spawn_subagent_with_optional_history(
            home,
            &mut app,
            child_sid,
            Some(&child_user_message_line(child_sid, prompt)),
        );

        // Exercise the live child notification path before its transcript is
        // reclaimed. The persisted completion snapshot deliberately carries no
        // output, matching the bridge; stdout belongs to the live MonitorEvent.
        assert!(handle_ext_notification(
            &make_task_backgrounded_notif(
                child_sid,
                "child-bg-tool-call",
                "child-bg-task",
                "printf 'persisted output'",
            ),
            &mut app,
        ));
        assert!(handle_ext_notification(
            &make_monitor_event_notif(child_sid, "child-bg-task", output),
            &mut app,
        ));
        assert_running_task(&app.agents[&AgentId(0)], child_sid, prompt, output);

        // Persist the canonical child transcript after the live notifications;
        // reopening below must reconstruct both rows from those disk records.
        let history = format!(
            "{}\n{}\n{}",
            child_user_message_line(child_sid, prompt),
            grow_history_line(child_sid, backgrounded),
            grow_history_line(child_sid, completed),
        );
        write_child_history_fixture(home, child_sid, &history);

        handle(
            make_ext_session_notification("sess-parent", test_subagent_finished(child_sid)),
            &mut app,
        );
        app.reclaim_finished_subagent_transcripts();
        assert!(
            app.agents[&AgentId(0)].subagent_views[child_sid]
                .scrollback
                .is_empty(),
            "reclamation should release the in-memory transcript rows"
        );

        // A late duplicate completion can arrive after reclamation. Its
        // pre-apply hydration must rebuild the canonical rows before applying
        // the notification, while keeping the existing task state owner.
        assert!(handle_ext_notification(
            &task_completed_notification(child_sid, "child-bg-task"),
            &mut app,
        ));
        assert_task_lifecycle(&app.agents[&AgentId(0)], child_sid, prompt, output);

        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(child_sid.into());
        close_subagent_fullscreen(app.agents.get_mut(&AgentId(0)).unwrap());
        app.reclaim_finished_subagent_transcripts();
        assert!(
            app.agents[&AgentId(0)].subagent_views[child_sid]
                .scrollback
                .is_empty()
        );

        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(child_sid.into());
        assert_task_lifecycle(&app.agents[&AgentId(0)], child_sid, prompt, output);

        // Repeated lifecycle notifications must neither append rows nor
        // replace the live task's stdout/cancellation owner with disk values.
        let retained_output = format!("{output}\nlive tail");
        {
            let task = app
                .agents
                .get_mut(&AgentId(0))
                .unwrap()
                .subagent_views
                .get_mut(child_sid)
                .unwrap()
                .session
                .bg_tasks
                .get_mut("child-bg-task")
                .unwrap();
            task.set_stdout(retained_output.clone());
            task.pending_kill = true;
            task.kill_requested_at = Some(std::time::Instant::now());
        }
        assert!(handle_ext_notification(
            &make_task_backgrounded_notif(
                child_sid,
                "child-bg-tool-call",
                "child-bg-task",
                "printf 'persisted output'",
            ),
            &mut app,
        ));
        assert!(handle_ext_notification(
            &task_completed_notification(child_sid, "child-bg-task"),
            &mut app,
        ));
        assert_task_lifecycle(
            &app.agents[&AgentId(0)],
            child_sid,
            prompt,
            &retained_output,
        );
        let task = &app.agents[&AgentId(0)].subagent_views[child_sid]
            .session
            .bg_tasks["child-bg-task"];
        assert!(
            task.pending_kill,
            "duplicate completion must retain the kill owner"
        );
        assert!(task.kill_requested_at.is_some());

        close_subagent_fullscreen(app.agents.get_mut(&AgentId(0)).unwrap());
        app.reclaim_finished_subagent_transcripts();
        assert!(
            app.agents[&AgentId(0)].subagent_views[child_sid]
                .scrollback
                .is_empty()
        );
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .open_subagent_fullscreen(child_sid.into());
        assert_task_lifecycle(
            &app.agents[&AgentId(0)],
            child_sid,
            prompt,
            &retained_output,
        );
        let task = &app.agents[&AgentId(0)].subagent_views[child_sid]
            .session
            .bg_tasks["child-bg-task"];
        assert!(
            task.pending_kill,
            "history replay must retain the kill owner"
        );
        assert!(task.kill_requested_at.is_some());
    });
}

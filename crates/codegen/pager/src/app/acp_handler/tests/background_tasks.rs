#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;
    use crate::app::session::BG_TASK_MAX_STDOUT;
    use std::time::Duration;

    /// Regression (resume sync): the on-disk replay stream re-emits persisted
    /// notifications through the generic `grow/session/update` envelope. A
    /// background `monitor`/bash task (`TaskBackgrounded`) must restore into
    /// `bg_tasks` on a resumed / second terminal — not be dropped by the
    /// default match arm — so the idle "watching" status line and the Tasks pane
    /// match the originating terminal. (Before this routing only subagents
    /// survived resume.)
    #[test]
    fn ext_session_update_replay_restores_bg_task() {
        let mut app = make_app_with_agent("sess-1");
        let id = AgentId(0);
        assert!(app.agents[&id].session.bg_tasks.is_empty());

        let update = GrowSessionUpdate::TaskBackgrounded {
            tool_call_id: "tc-mon".into(),
            task_id: "mon-1".into(),
            command: "tail -f deploy.log".into(),
            cwd: "/tmp".into(),
            output_file: "/tmp/mon-1.log".into(),
            monitor_description: Some("errors in deploy.log".into()),
            description: None,
        };
        handle(
            make_ext_session_notification_with_method("sess-1", "grow/session/update", update),
            &mut app,
        );

        let task = app.agents[&id]
            .session
            .bg_tasks
            .get("mon-1")
            .expect("replayed TaskBackgrounded must restore bg_tasks on resume");
        assert!(task.is_monitor, "monitor_description must mark is_monitor");
        assert_eq!(task.status, BgTaskStatus::Running);
    }

    /// Companion for scheduled `/loop`s: a replayed `ScheduledTaskCreated` must
    /// restore `scheduled_tasks`, and a later `ScheduledTaskDeleted` must net it
    /// back out — so a resumed terminal's loop count matches instead of staying
    /// empty until the next live fire. (Pairs with the shell-side persistence of
    /// these notifications in `notification_bridge.rs`.)
    #[test]
    fn ext_session_update_replay_restores_then_removes_scheduled_task() {
        let mut app = make_app_with_agent("sess-1");
        let id = AgentId(0);
        assert!(app.agents[&id].session.scheduled_tasks.is_empty());

        handle(
            make_ext_session_notification_with_method(
                "sess-1",
                "grow/session/update",
                GrowSessionUpdate::ScheduledTaskCreated {
                    task_id: "loop-1".into(),
                    prompt: "check deploy".into(),
                    human_schedule: "every 5 minutes".into(),
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                },
            ),
            &mut app,
        );
        assert!(
            app.agents[&id]
                .session
                .scheduled_tasks
                .contains_key("loop-1"),
            "replayed ScheduledTaskCreated must restore scheduled_tasks on resume"
        );

        handle(
            make_ext_session_notification_with_method(
                "sess-1",
                "grow/session/update",
                GrowSessionUpdate::ScheduledTaskDeleted {
                    task_id: "loop-1".into(),
                },
            ),
            &mut app,
        );
        assert!(
            app.agents[&id].session.scheduled_tasks.is_empty(),
            "replayed ScheduledTaskDeleted must remove the loop on resume"
        );
    }

    /// Regression: demotion path (foreground Execute → BgTask) must call
    /// finish_running() so the entry is removed from the running set.
    /// Without this, the entry stays orphaned as "running" forever.
    #[test]
    fn task_backgrounded_demotion_clears_running_state() {
        let mut app = make_app_with_agent("sess-1");
        let tc_id = "call-abc-123";

        setup_pending_execute_tool(&mut app, tc_id);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.scrollback.len(), 1);
        assert!(agent.scrollback.needs_animation());
        assert!(agent.session.tracker.pending_tool_entry_id(tc_id).is_some());

        let notif = make_task_backgrounded_notif("sess-1", tc_id, "task-001", "sleep 9999");
        let changed = handle_task_backgrounded(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            !agent.scrollback.needs_animation(),
            "entry must not be in the running set after demotion"
        );
        let entry = agent.scrollback.get(0).unwrap();
        assert!(
            matches!(entry.block, RenderBlock::BgTask(_)),
            "block should be demoted to BgTask"
        );
        assert!(!entry.is_running);
        assert!(agent.session.tracker.pending_tool_entry_id(tc_id).is_none());
    }

    /// Regression: late-detected is_background=true (raw_input arrives after the
    /// Execute block exists) followed by task_backgrounded must correctly demote
    /// the existing Execute block — not create a duplicate BgTask.
    #[test]
    fn task_backgrounded_late_detection_demotes_existing_entry() {
        let mut app = make_app_with_agent("sess-1");
        let tc_id = "call-late-bg-42";

        setup_pending_execute_tool(&mut app, tc_id);
        send_late_bg_detection(&mut app, tc_id);

        // Tool should be in BOTH pending_tools and bg_deferred_tools
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(agent.session.tracker.pending_tool_entry_id(tc_id).is_some());
        assert!(agent.session.tracker.bg_deferred_tools.contains_key(tc_id));

        let notif = make_task_backgrounded_notif("sess-1", tc_id, "task-late-001", "sleep 9999");
        let changed = handle_task_backgrounded(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.scrollback.len(), 1, "must not create duplicate");
        let entry = agent.scrollback.get(0).unwrap();
        assert!(matches!(entry.block, RenderBlock::BgTask(_)));
        assert!(!entry.is_running);
        assert!(!agent.scrollback.needs_animation());
        assert!(agent.session.tracker.pending_tool_entry_id(tc_id).is_none());
        assert!(!agent.session.tracker.bg_deferred_tools.contains_key(tc_id));
    }

    /// Regression: even when the wire notification carries its own
    /// `description`, the deferred-tool suppression key must still be drained
    /// (it is preferred but the entry otherwise leaks and keeps dropping late
    /// stdout updates for the rest of the session).
    #[test]
    fn task_backgrounded_with_wire_description_still_drains_deferred_tool() {
        let mut app = make_app_with_agent("sess-1");
        let tc_id = "call-late-bg-desc";

        setup_pending_execute_tool(&mut app, tc_id);
        send_late_bg_detection(&mut app, tc_id);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(agent.session.tracker.bg_deferred_tools.contains_key(tc_id));

        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-1"),
            update: GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: tc_id.into(),
                task_id: "task-late-desc".into(),
                command: "sleep 9999".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/output.log".into(),
                monitor_description: None,
                description: Some("Wait a while".into()),
            },
            meta: None,
        };
        let raw = serde_json::value::to_raw_value(&notif).unwrap();
        let notif = acp::ExtNotification::new("grow/task_backgrounded", raw.into());
        assert!(handle_task_backgrounded(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            !agent.session.tracker.bg_deferred_tools.contains_key(tc_id),
            "deferred-tool key must be drained even when wire description wins"
        );
    }

    /// Regression: a blank/whitespace wire `description` must not shadow a
    /// non-blank deferred (raw_input) description when there is no Execute
    /// block to recover from (fresh BgTask path). The label must come from the
    /// deferred description, not the blank wire value (which would otherwise
    /// fall back to the raw command).
    #[test]
    fn task_backgrounded_blank_wire_description_prefers_deferred() {
        let mut app = make_app_with_agent("sess-1");
        let tc_id = "call-blank-wire";

        // Simulate late is_background detection having stashed a real
        // description, with the placeholder entry dropped (no pending tool).
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent
                .session
                .tracker
                .bg_deferred_tools
                .insert(tc_id.to_string(), Some("deploy the server".to_string()));
        }
        assert!(
            app.agents
                .get(&AgentId(0))
                .unwrap()
                .session
                .tracker
                .pending_tool_entry_id(tc_id)
                .is_none(),
            "no Execute entry to recover from — exercises the merge chain"
        );

        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-1"),
            update: GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: tc_id.into(),
                task_id: "task-blank-wire".into(),
                command: "sleep 9999".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/output.log".into(),
                monitor_description: None,
                description: Some("   ".into()),
            },
            meta: None,
        };
        let raw = serde_json::value::to_raw_value(&notif).unwrap();
        let notif = acp::ExtNotification::new("grow/task_backgrounded", raw.into());
        assert!(handle_task_backgrounded(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            !agent.session.tracker.bg_deferred_tools.contains_key(tc_id),
            "deferred-tool key drained"
        );
        assert_eq!(agent.scrollback.len(), 1);
        match &agent.scrollback.get(0).unwrap().block {
            RenderBlock::BgTask(bg) => assert_eq!(
                bg.description.as_deref(),
                Some("deploy the server"),
                "blank wire description must not shadow the deferred description"
            ),
            other => panic!("expected BgTask, got {other:?}"),
        }
    }

    #[test]
    fn task_backgrounded_routes_to_child_session() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif =
            make_task_backgrounded_notif("child-sess", "tc-child-1", "task-child-1", "sleep 100");
        let changed = handle_task_backgrounded(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        // Parent scrollback must NOT have the bg task block.
        assert_eq!(agent.scrollback.len(), 0, "parent scrollback must be empty");
        assert!(
            agent.session.bg_tasks.is_empty(),
            "parent session must not have the bg task"
        );

        // Child view must have the bg task.
        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(child.scrollback.len(), 1);
        assert!(child.session.bg_tasks.contains_key("task-child-1"));
        assert!(
            child
                .session
                .bg_tool_call_to_task
                .contains_key("tc-child-1")
        );
    }

    #[test]
    fn task_backgrounded_root_still_routes_to_parent() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif =
            make_task_backgrounded_notif("parent-sess", "tc-root-1", "task-root-1", "ls -la");
        let changed = handle_task_backgrounded(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.scrollback.len(), 1);
        assert!(agent.session.bg_tasks.contains_key("task-root-1"));

        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(
            child.scrollback.len(),
            0,
            "child scrollback must not be affected"
        );
    }

    #[test]
    fn task_backgrounded_monitor_prefix_marks_is_monitor() {
        // Reparented monitor / older backend: the command carries the
        // "[monitor] <desc>" prefix but the notification has no
        // monitor_description. The pager must still mark it a monitor and use
        // the stripped text as the description so it renders as a Monitor row.
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif = make_task_backgrounded_notif(
            "parent-sess",
            "tc-mon-1",
            "task-mon-1",
            "[monitor] event counter",
        );
        handle_task_backgrounded(&notif, &mut app);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let task = agent
            .session
            .bg_tasks
            .get("task-mon-1")
            .expect("bg task registered");
        assert!(
            task.is_monitor,
            "a `[monitor] ` command prefix should mark the task as a monitor"
        );
        assert_eq!(task.description.as_deref(), Some("event counter"));
    }

    /// Resume regression: the agent's cold-load reconciliation
    /// completes replay-restored dead tasks with `signal: "session_restart"`.
    /// That synthetic completion must finalize state QUIETLY — finish the
    /// replayed "Task started" entry and mark the task not-running — without
    /// pushing a fresh red "Task failed" block into the resumed scrollback.
    #[test]
    fn session_restart_completion_finalizes_without_failure_block() {
        let mut app = make_app_with_agent("sess-1");

        let replayed =
            make_replayed_task_backgrounded_notif("sess-1", "tc-r", "task-r", "tail -f x.log");
        handle_task_backgrounded(&replayed, &mut app);
        {
            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(agent.scrollback.len(), 1, "replay restores started block");
            assert!(agent.scrollback.needs_animation(), "restored row runs");
        }

        let notif = make_task_completed_notif_with_signal(
            "sess-1",
            "task-r",
            "tail -f x.log",
            None,
            Some("session_restart"),
        );
        let changed = handle_task_completed(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            1,
            "stale-on-load completion must not push a 'Task failed' block"
        );
        assert!(
            !agent.scrollback.needs_animation(),
            "the replayed started entry must be finished (no running accent)"
        );
        let task = &agent.session.bg_tasks["task-r"];
        assert_eq!(
            task.status,
            BgTaskStatus::Failed,
            "state still records the task as not running"
        );
    }

    /// Guard that the quiet path is NARROW: any other kill signal keeps the
    /// live behavior of pushing a completion/failure block.
    #[test]
    fn non_restart_signal_completion_still_pushes_failure_block() {
        let mut app = make_app_with_agent("sess-1");

        let bg = make_task_backgrounded_notif("sess-1", "tc-k", "task-k", "sleep 999");
        handle_task_backgrounded(&bg, &mut app);

        let notif = make_task_completed_notif_with_signal(
            "sess-1",
            "task-k",
            "sleep 999",
            None,
            Some("SIGKILL"),
        );
        handle_task_completed(&notif, &mut app);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            2,
            "a real kill must still render started + failed blocks"
        );
    }

    #[test]
    fn task_completed_routes_to_child_session() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        // First, background a task on the child.
        let bg_notif =
            make_task_backgrounded_notif("child-sess", "tc-child-2", "task-child-2", "echo hi");
        handle_task_backgrounded(&bg_notif, &mut app);

        // Now complete it.
        let notif = make_task_completed_notif("child-sess", "task-child-2", "echo hi", Some(0));
        let changed = handle_task_completed(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        // Parent must NOT have a completion block.
        assert_eq!(
            agent.scrollback.len(),
            0,
            "parent scrollback must not have completion block"
        );

        // Child must have both the started and completed blocks.
        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(child.scrollback.len(), 2, "child: started + completed");
        let bg = child.session.bg_tasks.get("task-child-2").unwrap();
        assert!(matches!(bg.status, BgTaskStatus::Done));
    }

    #[test]
    fn task_completed_root_still_routes_to_parent() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        // Background and complete a task on the parent.
        let bg_notif =
            make_task_backgrounded_notif("parent-sess", "tc-root-2", "task-root-2", "echo root");
        handle_task_backgrounded(&bg_notif, &mut app);
        let notif = make_task_completed_notif("parent-sess", "task-root-2", "echo root", Some(0));
        let changed = handle_task_completed(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            2,
            "parent: started + completed blocks"
        );
        let bg = agent.session.bg_tasks.get("task-root-2").unwrap();
        assert!(matches!(bg.status, BgTaskStatus::Done));

        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(
            child.scrollback.len(),
            0,
            "child scrollback must not be affected"
        );
    }

    #[test]
    fn task_completed_failure_routes_to_child_session() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        let bg_notif = make_task_backgrounded_notif("child-sess", "tc-fail", "task-fail", "exit 1");
        handle_task_backgrounded(&bg_notif, &mut app);

        let notif = make_task_completed_notif("child-sess", "task-fail", "exit 1", Some(1));
        let changed = handle_task_completed(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            0,
            "parent scrollback must not have failure block"
        );

        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(child.scrollback.len(), 2, "child: started + failed");
        let bg = child.session.bg_tasks.get("task-fail").unwrap();
        assert!(matches!(bg.status, BgTaskStatus::Failed));
        assert_eq!(bg.exit_code, Some(1));
    }

    #[test]
    fn monitor_event_routes_to_child_session() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        // Background a task on the child so monitor event has somewhere to land.
        let bg_notif =
            make_task_backgrounded_notif("child-sess", "tc-child-3", "task-child-3", "tail -f");
        handle_task_backgrounded(&bg_notif, &mut app);

        let notif = make_monitor_event_notif("child-sess", "task-child-3", "new log line");
        let changed = handle_monitor_event(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.bg_tasks.is_empty(),
            "parent must not have the bg task"
        );

        let child = agent.subagent_views.get("child-sess").unwrap();
        let task = child.session.bg_tasks.get("task-child-3").unwrap();
        assert_eq!(task.stdout, "new log line");
    }

    #[test]
    fn monitor_event_root_still_routes_to_parent() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        // Background a task on the parent.
        let bg_notif =
            make_task_backgrounded_notif("parent-sess", "tc-root-3", "task-root-3", "tail -f");
        handle_task_backgrounded(&bg_notif, &mut app);

        let notif = make_monitor_event_notif("parent-sess", "task-root-3", "root event");
        let changed = handle_monitor_event(&notif, &mut app);
        assert!(changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let task = agent.session.bg_tasks.get("task-root-3").unwrap();
        assert_eq!(task.stdout, "root event");

        let child = agent.subagent_views.get("child-sess").unwrap();
        assert!(
            child.session.bg_tasks.is_empty(),
            "child must not have the bg task"
        );
    }

    #[test]
    fn task_backgrounded_child_inactive_returns_false_but_mutates_state() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        // Insert a second agent and switch to it so the first agent is inactive.
        let other = make_agent(Some("other-sess"));
        app.agents.insert(AgentId(1), other);
        crate::app::root::dispatch::switch_to_agent(
            &mut app,
            AgentId(1),
            crate::app::root::dispatch::SwitchCause::New,
        );
        assert!(matches!(app.active_view, ActiveView::Agent(AgentId(1))));

        let notif =
            make_task_backgrounded_notif("child-sess", "tc-bg-inact", "task-bg-inact", "sleep 1");
        let changed = handle_task_backgrounded(&notif, &mut app);
        // Active view was NOT affected — should return false.
        assert!(!changed);

        // But the bg task state must still land in the child view.
        let agent = app.agents.get(&AgentId(0)).unwrap();
        let child = agent.subagent_views.get("child-sess").unwrap();
        assert!(child.session.bg_tasks.contains_key("task-bg-inact"));
        assert_eq!(child.scrollback.len(), 1);
    }

    #[test]
    fn task_completed_child_inactive_returns_false_but_mutates_state() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        // Background a task on the child first.
        let bg_notif = make_task_backgrounded_notif(
            "child-sess",
            "tc-compl-inact",
            "task-compl-inact",
            "echo",
        );
        handle_task_backgrounded(&bg_notif, &mut app);

        // Now switch away.
        let other = make_agent(Some("other-sess"));
        app.agents.insert(AgentId(1), other);
        crate::app::root::dispatch::switch_to_agent(
            &mut app,
            AgentId(1),
            crate::app::root::dispatch::SwitchCause::New,
        );

        let notif = make_task_completed_notif("child-sess", "task-compl-inact", "echo", Some(0));
        let changed = handle_task_completed(&notif, &mut app);
        assert!(!changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let child = agent.subagent_views.get("child-sess").unwrap();
        let bg = child.session.bg_tasks.get("task-compl-inact").unwrap();
        assert!(matches!(bg.status, BgTaskStatus::Done));
        assert_eq!(child.scrollback.len(), 2);
    }

    #[test]
    fn task_backgrounded_unknown_session_returns_false() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif =
            make_task_backgrounded_notif("unknown-sess", "tc-unknown", "task-unknown", "sleep 1");
        let changed = handle_task_backgrounded(&notif, &mut app);
        assert!(!changed);
    }

    #[test]
    fn task_completed_unknown_session_returns_false() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif = make_task_completed_notif("unknown-sess", "task-unknown", "echo x", Some(0));
        let changed = handle_task_completed(&notif, &mut app);
        assert!(!changed);
    }

    #[test]
    fn monitor_event_unknown_session_returns_false() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");
        let notif = make_monitor_event_notif("unknown-sess", "task-unknown", "event");
        let changed = handle_monitor_event(&notif, &mut app);
        assert!(!changed);
    }

    #[test]
    fn replayed_task_backgrounded_marks_restored_from_replay() {
        let mut app = make_app_with_agent("sess-1");

        let replayed =
            make_replayed_task_backgrounded_notif("sess-1", "tc-r", "task-r", "tail -f x.log");
        handle_task_backgrounded(&replayed, &mut app);
        let live = make_task_backgrounded_notif("sess-1", "tc-l", "task-l", "sleep 5");
        handle_task_backgrounded(&live, &mut app);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.bg_tasks["task-r"].restored_from_replay,
            "isReplay-stamped TaskBackgrounded must mark restored_from_replay"
        );
        assert!(
            !agent.session.bg_tasks["task-l"].restored_from_replay,
            "live TaskBackgrounded must not mark restored_from_replay"
        );
    }

    /// Paged full-load state-only replay: the start fact restores the exact
    /// task/tool-call identity into the Tasks center WITHOUT drawing any
    /// scrollback — the transcript body belongs to HistoryRow pagination.
    #[test]
    fn state_only_start_restores_row_identity_without_scrollback() {
        let mut app = make_app_with_agent("sess-1");

        let notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-so-1".into(),
                task_id: "task-so-1".into(),
                command: "cargo build --release".into(),
                cwd: "/work".into(),
                output_file: "/work/build.log".into(),
                monitor_description: None,
                description: Some("compile in release mode".into()),
            },
        );
        assert!(handle_task_backgrounded(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            0,
            "state-only start must not draw a scrollback block"
        );
        let task = agent.session.bg_tasks.get("task-so-1").expect("row restored");
        assert_eq!(task.tool_call_id, "tc-so-1");
        assert_eq!(task.command, "cargo build --release");
        assert_eq!(task.cwd, "/work");
        assert_eq!(task.output_file, "/work/build.log");
        assert_eq!(task.description.as_deref(), Some("compile in release mode"));
        assert_eq!(task.status, BgTaskStatus::Running);
        assert!(task.restored_from_replay, "row is restored historical context");
        assert_eq!(
            agent.session.bg_tool_call_to_task.get("tc-so-1").map(String::as_str),
            Some("task-so-1"),
            "stdout routing must be established from the start fact"
        );
    }

    /// Monitor detection mirrors the scrollback path so a restored monitor
    /// renders with the Monitor tag, not bash highlighting.
    #[test]
    fn state_only_start_marks_monitor_rows() {
        let mut app = make_app_with_agent("sess-1");
        let notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-mon-so".into(),
                task_id: "task-mon-so".into(),
                command: "tail -f deploy.log".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/mon.log".into(),
                monitor_description: Some("errors in deploy.log".into()),
                description: None,
            },
        );
        assert!(handle_task_backgrounded(&notif, &mut app));

        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-mon-so"];
        assert!(task.is_monitor, "monitor_description must mark is_monitor");
        assert_eq!(task.description.as_deref(), Some("errors in deploy.log"));

        // Reparented / older backends bake the "[monitor] " prefix into the
        // command; the state-only path must still detect it.
        let mut app = make_app_with_agent("sess-1");
        let notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-mon-pfx".into(),
                task_id: "task-mon-pfx".into(),
                command: "[monitor] event counter".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/mon.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        assert!(handle_task_backgrounded(&notif, &mut app));
        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-mon-pfx"];
        assert!(task.is_monitor, "command prefix must mark is_monitor");
        assert_eq!(task.description.as_deref(), Some("event counter"));
    }

    /// The paired completion settles the restored Running row with the
    /// snapshot's real start/end/exit facts and final stdout — the full
    /// recorded interval, not "elapsed since load".
    #[test]
    fn state_only_completion_settles_with_snapshot_facts() {
        let mut app = make_app_with_agent("sess-1");
        let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let end = start + Duration::from_secs(125);
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-settle".into(),
                task_id: "task-settle".into(),
                command: "long-build".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        assert!(handle_task_backgrounded(&start_notif, &mut app));

        let snapshot = settled_task_snapshot(
            "task-settle",
            "long-build",
            start,
            end,
            Some(0),
            None,
            "line one\nline two\nline three",
            false,
        );
        let mut snapshot = snapshot;
        snapshot.description = Some("the long build".into());
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.scrollback.len(),
            0,
            "state-only completion must not draw a completion block"
        );
        let task = agent.session.bg_tasks.get("task-settle").expect("row present");
        assert_eq!(task.status, BgTaskStatus::Done);
        assert_eq!(task.exit_code, Some(0));
        assert_eq!(task.start_time, start, "real start must replace the provisional one");
        assert_eq!(task.end_time, Some(end), "real end must be recorded");
        assert_eq!(task.elapsed(), Duration::from_secs(125));
        assert_eq!(task.stdout, "line one\nline two\nline three");
        assert_eq!(task.stdout_line_count, 3);
        assert!(!task.truncated);
        assert_eq!(task.description.as_deref(), Some("the long build"));
        assert!(!task.pending_kill);
    }

    /// A signal-only (or non-zero) exit settles as Failed and keeps the
    /// recorded signal.
    #[test]
    fn state_only_completion_records_failure_and_signal() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-fail-so".into(),
                task_id: "task-fail-so".into(),
                command: "crashy".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);

        let snapshot = settled_task_snapshot(
            "task-fail-so",
            "crashy",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(7),
            None,
            Some("SIGTERM"),
            "",
            false,
        );
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-fail-so"];
        assert_eq!(task.status, BgTaskStatus::Failed);
        assert_eq!(task.exit_code, None);
        assert_eq!(task.signal.as_deref(), Some("SIGTERM"));
    }

    /// Duplicate (or repeated) state-only completions are idempotent: the
    /// settled row is never recomputed from an older snapshot.
    #[test]
    fn state_only_completion_is_idempotent() {
        let mut app = make_app_with_agent("sess-1");
        let start = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(5_000);
        let end = start + Duration::from_secs(60);
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-idem".into(),
                task_id: "task-idem".into(),
                command: "repeat-me".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        let completion = |app: &mut AppView| {
            let snapshot = settled_task_snapshot(
                "task-idem",
                "repeat-me",
                start,
                end,
                Some(0),
                None,
                "settled output",
                false,
            );
            handle_task_completed(
                &make_state_only_grow_task_notification(
                    "sess-1",
                    GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
                ),
                app,
            )
        };
        assert!(completion(&mut app), "first settle adopts the snapshot");

        let settled = app.agents[&AgentId(0)].session.bg_tasks["task-idem"].clone();
        assert!(
            !completion(&mut app),
            "a completion for an already-settled row must not re-apply"
        );
        let after = &app.agents[&AgentId(0)].session.bg_tasks["task-idem"];
        assert_eq!(after.status, settled.status);
        assert_eq!(after.start_time, settled.start_time);
        assert_eq!(after.end_time, settled.end_time);
        assert_eq!(after.stdout, settled.stdout);
    }

    /// A replayed start must never rewind a row that already reached its
    /// terminal state (e.g. the completion fact arrived before the start is
    /// re-delivered, or the burst repeats).
    #[test]
    fn state_only_start_keeps_terminal_row() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-term".into(),
                task_id: "task-term".into(),
                command: "done-cmd".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        let end = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(9_000);
        let snapshot =
            settled_task_snapshot("task-term", "done-cmd", end - Duration::from_secs(9), end, Some(0), None, "", false);
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        handle_task_completed(&completion, &mut app);
        let settled = app.agents[&AgentId(0)].session.bg_tasks["task-term"].clone();

        let repeated_start = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-term".into(),
                task_id: "task-term".into(),
                command: "done-cmd".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        assert!(
            !handle_task_backgrounded(&repeated_start, &mut app),
            "duplicate start for an existing row must not report new activity"
        );
        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-term"];
        assert_eq!(task.status, BgTaskStatus::Done, "start must not rewind a terminal row");
        assert_eq!(task.start_time, settled.start_time);
        assert_eq!(task.end_time, settled.end_time);
        assert_eq!(
            app.agents[&AgentId(0)]
                .session
                .bg_tool_call_to_task
                .get("tc-term")
                .map(String::as_str),
            Some("task-term"),
            "stdout routing stays refreshed"
        );
    }

    /// A pending kill is a live fact of this client: a replayed start must
    /// not roll the row back (kill state and live output survive untouched).
    #[test]
    fn state_only_start_keeps_live_kill_facts() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-kill".into(),
                task_id: "task-kill".into(),
                command: "sleep 999".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        let (live_start, live_stdout_line_count) = {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            let task = agent.session.bg_tasks.get_mut("task-kill").unwrap();
            task.set_stdout("live output".into());
            task.pending_kill = true;
            task.kill_requested_at = Some(std::time::Instant::now());
            (task.start_time, task.stdout_line_count)
        };

        let repeated_start = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-kill".into(),
                task_id: "task-kill".into(),
                command: "sleep 999".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        assert!(!handle_task_backgrounded(&repeated_start, &mut app));
        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-kill"];
        assert_eq!(task.status, BgTaskStatus::Running);
        assert!(task.pending_kill, "kill request must survive the replayed start");
        assert!(task.kill_requested_at.is_some());
        assert_eq!(task.stdout, "live output", "live stdout must survive");
        assert_eq!(task.stdout_line_count, live_stdout_line_count);
        assert_eq!(task.start_time, live_start);
    }

    /// Retained non-empty live stdout wins: the settlement snapshot must not
    /// overwrite output this client already holds. `truncated` only ORs in.
    #[test]
    fn state_only_completion_keeps_retained_stdout() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-retain".into(),
                task_id: "task-retain".into(),
                command: "streamer".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .bg_tasks
            .get_mut("task-retain")
            .unwrap()
            .set_stdout("live streamed output".into());

        let snapshot = settled_task_snapshot(
            "task-retain",
            "streamer",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(3),
            Some(0),
            None,
            "older snapshot output",
            true,
        );
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-retain"];
        assert_eq!(
            task.stdout, "live streamed output",
            "retained non-empty stdout must win over the snapshot"
        );
        assert_eq!(task.stdout_line_count, 1);
        assert!(task.truncated, "snapshot truncated must OR in");
        assert_eq!(task.status, BgTaskStatus::Done, "the row still settles");
    }

    /// An empty live buffer is filled from the settlement snapshot (via
    /// `set_stdout`, keeping cap/line-count invariants), and a live
    /// truncated flag is never cleared by a non-truncated snapshot.
    #[test]
    fn state_only_completion_fills_empty_stdout_and_ors_truncation() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-fill".into(),
                task_id: "task-fill".into(),
                command: "printer".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .bg_tasks
            .get_mut("task-fill")
            .unwrap()
            .truncated = true;

        let snapshot = settled_task_snapshot(
            "task-fill",
            "printer",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(2),
            Some(0),
            None,
            "snapshot line A\nsnapshot line B",
            false,
        );
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-fill"];
        assert_eq!(task.stdout, "snapshot line A\nsnapshot line B");
        assert_eq!(task.stdout_line_count, 2);
        assert!(
            task.truncated,
            "a live truncated flag must never be cleared by the snapshot"
        );
    }

    /// An oversized snapshot output goes through `set_stdout`, so the row
    /// keeps the `BG_TASK_MAX_STDOUT` cap and flips `truncated`.
    #[test]
    fn state_only_completion_caps_oversized_snapshot_stdout() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-cap".into(),
                task_id: "task-cap".into(),
                command: "spewer".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);

        let oversized = "x".repeat(BG_TASK_MAX_STDOUT + 1);
        let snapshot = settled_task_snapshot(
            "task-cap",
            "spewer",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(4),
            Some(0),
            None,
            &oversized,
            false,
        );
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let task = &app.agents[&AgentId(0)].session.bg_tasks["task-cap"];
        assert_eq!(task.stdout.len(), BG_TASK_MAX_STDOUT);
        assert!(task.truncated);
        assert_eq!(task.stdout_line_count, 1);
    }

    /// A completion without a restored start row has no task/tool-call
    /// identity to adopt: the shell projects the complete visible lifecycle,
    /// so the update is dropped instead of fabricating a placeholder row.
    #[test]
    fn state_only_completion_without_start_row_is_dropped() {
        let mut app = make_app_with_agent("sess-1");
        let snapshot = settled_task_snapshot(
            "task-orphan",
            "no-start",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1),
            Some(0),
            None,
            "output",
            false,
        );
        let completion = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(!handle_task_completed(&completion, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.bg_tasks.is_empty(),
            "no placeholder row may be created without a start identity"
        );
        assert!(agent.session.bg_tool_call_to_task.is_empty());
        assert_eq!(agent.scrollback.len(), 0);
    }

    /// An unsettled restored start stays Running so the ordinary (non-replay)
    /// stale reconciliation after load finds the row and finalizes it
    /// quietly — no completion block is pushed for the pre-load death.
    #[test]
    fn unsettled_state_only_start_settles_via_stale_reconciliation() {
        let mut app = make_app_with_agent("sess-1");
        let start_notif = make_state_only_grow_task_notification(
            "sess-1",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-stale".into(),
                task_id: "task-stale".into(),
                command: "died-with-session".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        handle_task_backgrounded(&start_notif, &mut app);
        assert_eq!(
            app.agents[&AgentId(0)].session.bg_tasks["task-stale"].status,
            BgTaskStatus::Running,
            "unsettled start must stay Running for reconciliation"
        );

        // Ordinary stale reconciliation (live envelope, no replay marks),
        // exactly as the shell emits after a cold load.
        let notif = make_task_completed_notif_with_signal(
            "sess-1",
            "task-stale",
            "died-with-session",
            None,
            Some("session_restart"),
        );
        assert!(handle_task_completed(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let task = &agent.session.bg_tasks["task-stale"];
        assert_eq!(task.status, BgTaskStatus::Failed);
        assert_eq!(
            agent.scrollback.len(),
            0,
            "stale reconciliation must not push a failure block for a row without scrollback"
        );
    }

    /// State-only replay must not present its cursor as the first applied
    /// history item (it carries no transcript body); an ordinary replayed
    /// task update keeps recording it.
    #[test]
    fn state_only_task_replay_does_not_note_history_cursor() {
        const CHILD: &str = "GROW_TASK_STATE_CURSOR_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let home = tempfile::tempdir().unwrap();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "app::acp_handler::tests::background_tasks::state_only_task_replay_does_not_note_history_cursor",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(CHILD, "1")
                .env("GROW_INSTRUMENTATION", "log")
                .env("GROW_HOME", home.path())
                .output()
                .unwrap();
            assert!(
                output.status.success()
                    && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
                "{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        assert_ne!(
            diagnostics::instrumentation::current_mode(),
            diagnostics::instrumentation::InstrumentationMode::Disabled
        );
        let mut app = make_app_with_agent("sess-1");
        app.agents.get_mut(&AgentId(0)).unwrap().session.loading_replay = true;

        // State-only start with a cursor: applied, but the cursor must not
        // become the first applied history item.
        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-1"),
            update: GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-cursor".into(),
                task_id: "task-cursor".into(),
                command: "sleep 1".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
            meta: Some(serde_json::json!({
                "isReplay": true,
                "grow/taskStateOnly": true,
                "grow/historyCursor": "state-only-cursor",
            })),
        };
        let raw = serde_json::value::to_raw_value(&notif).unwrap();
        let notif = acp::ExtNotification::new("grow/session/update", raw.into());
        assert!(handle_ext_notification(&notif, &mut app));
        assert!(
            app.agents[&AgentId(0)]
                .session
                .replay_first_applied
                .is_none(),
            "state-only replay must not note_history_applied"
        );

        // Ordinary replayed task update (full replay projection): the cursor
        // keeps being recorded.
        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-1"),
            update: GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-cursor-2".into(),
                task_id: "task-cursor-2".into(),
                command: "sleep 2".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
            meta: Some(serde_json::json!({
                "isReplay": true,
                "grow/historyCursor": "ordinary-cursor",
            })),
        };
        let raw = serde_json::value::to_raw_value(&notif).unwrap();
        let notif = acp::ExtNotification::new("grow/session/update", raw.into());
        assert!(handle_ext_notification(&notif, &mut app));
        assert_eq!(
            app.agents[&AgentId(0)].session.replay_first_applied.as_deref(),
            Some("ordinary-cursor"),
            "ordinary replay keeps recording the first applied cursor"
        );
    }

    /// The state-only path requires BOTH marks. `isReplay` alone keeps the
    /// full replay projection (scrollback block), and `grow/taskStateOnly`
    /// alone is not a replay (live path).
    #[test]
    fn partial_marks_keep_original_paths() {
        // isReplay only → existing full replay path draws the block.
        let mut app = make_app_with_agent("sess-1");
        let replayed = make_replayed_task_backgrounded_notif("sess-1", "tc-re", "task-re", "cmd");
        assert!(handle_task_backgrounded(&replayed, &mut app));
        {
            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(agent.scrollback.len(), 1, "full replay still projects the block");
            assert!(agent.session.bg_tasks["task-re"].restored_from_replay);
        }

        // taskStateOnly without isReplay → live path draws the block.
        let mut app = make_app_with_agent("sess-1");
        let notif = SessionNotification {
            session_id: acp::SessionId::new("sess-1"),
            update: GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-half".into(),
                task_id: "task-half".into(),
                command: "cmd".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
            meta: Some(serde_json::json!({
                "grow/taskStateOnly": true,
            })),
        };
        let raw = serde_json::value::to_raw_value(&notif).unwrap();
        let notif = acp::ExtNotification::new("grow/session/update", raw.into());
        assert!(handle_task_backgrounded(&notif, &mut app));
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.scrollback.len(), 1, "taskStateOnly alone is not a replay");
        assert!(
            !agent.session.bg_tasks["task-half"].restored_from_replay,
            "without isReplay the row takes the live path"
        );
    }

    /// State-only replay routes to the owning child session: the child's
    /// Tasks center is restored and neither the parent state nor any
    /// scrollback is touched.
    #[test]
    fn state_only_replay_routes_to_child_session() {
        let mut app = make_app_with_parent_and_child("parent-sess", "child-sess");

        let start_notif = make_state_only_grow_task_notification(
            "child-sess",
            GrowSessionUpdate::TaskBackgrounded {
                tool_call_id: "tc-child-so".into(),
                task_id: "task-child-so".into(),
                command: "sleep 30".into(),
                cwd: "/tmp".into(),
                output_file: "/tmp/out.log".into(),
                monitor_description: None,
                description: None,
            },
        );
        assert!(handle_task_backgrounded(&start_notif, &mut app));
        let snapshot = settled_task_snapshot(
            "task-child-so",
            "sleep 30",
            std::time::SystemTime::UNIX_EPOCH,
            std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(30),
            Some(0),
            None,
            "",
            false,
        );
        let completion = make_state_only_grow_task_notification(
            "child-sess",
            GrowSessionUpdate::TaskCompleted { task_snapshot: snapshot },
        );
        assert!(handle_task_completed(&completion, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.bg_tasks.is_empty(),
            "parent must not adopt the child's task"
        );
        assert_eq!(agent.scrollback.len(), 0, "parent scrollback untouched");
        let child = agent.subagent_views.get("child-sess").unwrap();
        assert_eq!(child.scrollback.len(), 0, "child scrollback untouched");
        let task = child.session.bg_tasks.get("task-child-so").expect("child row restored");
        assert_eq!(task.status, BgTaskStatus::Done);
        assert_eq!(task.elapsed(), Duration::from_secs(30));
    }


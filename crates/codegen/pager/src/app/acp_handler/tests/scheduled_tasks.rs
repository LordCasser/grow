#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn fired_known_task_updates_next_fire_at_only() {
        let mut app = make_app_with_agent("sess-1");
        let original_created_at = Instant::now() - std::time::Duration::from_secs(60);
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.scheduled_tasks.insert(
                "task-1".into(),
                crate::app::agent::ScheduledTaskInfo {
                    task_id: "task-1".into(),
                    prompt: "original prompt".into(),
                    human_schedule: "every 5 minutes".into(),
                    created_at: original_created_at,
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                    tag: "loop".into(),
                    last_subagent_id: None,
                },
            );
        }

        let notif = make_fired_notif(
            "sess-1",
            "task-1",
            // Different field values to verify they are NOT copied over.
            "DIFFERENT",
            "every 1 hour",
            Some("2026-02-02T02:02:02Z"),
        );
        assert!(handle_scheduled_task_fired(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.scheduled_tasks.get("task-1").unwrap();
        assert_eq!(info.prompt, "original prompt", "prompt must not change");
        assert_eq!(
            info.human_schedule, "every 5 minutes",
            "human_schedule must not change"
        );
        assert_eq!(
            info.created_at, original_created_at,
            "created_at must not change"
        );
        assert_eq!(
            info.next_fire_at.as_deref(),
            Some("2026-02-02T02:02:02Z"),
            "next_fire_at must be updated"
        );
        assert_eq!(
            agent.session.scheduled_tasks.len(),
            1,
            "no extra entry should be inserted"
        );
    }

    #[test]
    fn fired_unknown_task_inserts_entry_from_payload() {
        let mut app = make_app_with_agent("sess-1");
        let before = Instant::now();
        let notif = make_fired_notif(
            "sess-1",
            "task-new",
            "scheduled prompt",
            "every 10 minutes",
            Some("2026-03-03T03:03:03Z"),
        );
        assert!(handle_scheduled_task_fired(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.session.scheduled_tasks.len(), 1);
        let info = agent.session.scheduled_tasks.get("task-new").unwrap();
        assert_eq!(info.task_id, "task-new");
        assert_eq!(info.prompt, "scheduled prompt");
        assert_eq!(info.human_schedule, "every 10 minutes");
        assert_eq!(info.next_fire_at.as_deref(), Some("2026-03-03T03:03:03Z"));
        assert!(
            info.created_at >= before && info.created_at <= Instant::now(),
            "created_at should be set to roughly now"
        );
    }

    #[test]
    fn fired_unknown_task_with_none_next_fire_skips_insert() {
        // Mirrors handle_missed_tasks output: a missed one-shot fires with
        // next_fire_at: None and is immediately removed. The pane should not
        // flicker an entry that the Removed will instantly drop.
        let mut app = make_app_with_agent("sess-1");
        let notif = make_fired_notif(
            "sess-1",
            "missed-1",
            "missed prompt",
            "every 1 minute",
            None,
        );
        assert!(handle_scheduled_task_fired(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.scheduled_tasks.is_empty(),
            "no entry should be inserted for a Vacant + None fire"
        );
    }

    #[test]
    fn fired_known_task_with_none_next_fire_clears_field() {
        // The Vacant short-circuit on next_fire_at: None must NOT apply to
        // Occupied — clearing an existing countdown is correct behaviour.
        let mut app = make_app_with_agent("sess-1");
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.scheduled_tasks.insert(
                "task-1".into(),
                crate::app::agent::ScheduledTaskInfo {
                    task_id: "task-1".into(),
                    prompt: "p".into(),
                    human_schedule: "every 1 minute".into(),
                    created_at: Instant::now(),
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                    tag: "loop".into(),
                    last_subagent_id: None,
                },
            );
        }
        let notif = make_fired_notif("sess-1", "task-1", "p", "every 1 minute", None);
        assert!(handle_scheduled_task_fired(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.scheduled_tasks.get("task-1").unwrap();
        assert!(info.next_fire_at.is_none());
    }

    #[test]
    fn fired_updates_correct_agent_when_active_view_differs() {
        let mut app = make_app_two_agents();
        // Seed a known task on agent 0.
        {
            let agent0 = app.agents.get_mut(&AgentId(0)).unwrap();
            agent0.session.scheduled_tasks.insert(
                "task-owner".into(),
                crate::app::agent::ScheduledTaskInfo {
                    task_id: "task-owner".into(),
                    prompt: "check PR".into(),
                    human_schedule: "every 5m".into(),
                    created_at: Instant::now(),
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                    tag: "loop".into(),
                    last_subagent_id: None,
                },
            );
        }

        // Fire notification targets agent 0's session, but active_view
        // points to agent 1. Return value is "needs redraw" — false is
        // correct when the mutated agent is not the active view.
        let notif = make_fired_notif(
            "sess-owner",
            "task-owner",
            "check PR",
            "every 5m",
            Some("2026-06-01T12:00:00Z"),
        );
        let needs_redraw = handle_scheduled_task_fired(&notif, &mut app);
        assert!(
            !needs_redraw,
            "non-active agent mutation should not trigger redraw"
        );

        // Agent 0's next_fire_at must be updated.
        let agent0 = app.agents.get(&AgentId(0)).unwrap();
        let info = agent0.session.scheduled_tasks.get("task-owner").unwrap();
        assert_eq!(
            info.next_fire_at.as_deref(),
            Some("2026-06-01T12:00:00Z"),
            "next_fire_at must update on the owning agent, not the active one"
        );

        // Agent 1 must be completely untouched.
        let agent1 = app.agents.get(&AgentId(1)).unwrap();
        assert!(
            agent1.session.scheduled_tasks.is_empty(),
            "non-owning agent must not receive the update"
        );
    }

    #[test]
    fn every_fire_links_chip_to_latest_detached_subagent() {
        let mut app = make_app_with_agent("sess-1");

        let notif = make_fired_notif_with_subagent("sess-1", "task-bg", "sub-abc");
        assert!(handle_scheduled_task_fired(&notif, &mut app));
        {
            let agent = app.agents.get(&AgentId(0)).unwrap();
            let info = agent.session.scheduled_tasks.get("task-bg").unwrap();
            assert_eq!(info.last_subagent_id.as_deref(), Some("sub-abc"));
        }

        let notif = make_fired_notif_with_subagent("sess-1", "task-bg", "sub-def");
        assert!(handle_scheduled_task_fired(&notif, &mut app));
        {
            let agent = app.agents.get(&AgentId(0)).unwrap();
            let info = agent.session.scheduled_tasks.get("task-bg").unwrap();
            assert_eq!(info.last_subagent_id.as_deref(), Some("sub-def"));
        }

        let notif = make_fired_notif(
            "sess-1",
            "task-bg",
            "p",
            "every 1 minute",
            Some("2026-03-03T03:03:03Z"),
        );
        assert!(handle_scheduled_task_fired(&notif, &mut app));
        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.scheduled_tasks.get("task-bg").unwrap();
        assert_eq!(info.last_subagent_id.as_deref(), Some("subagent-1"));
    }

    #[test]
    fn created_upserts_existing_chip_preserving_identity_and_linkage() {
        let mut app = make_app_with_agent("sess-1");
        let original_created_at = Instant::now() - std::time::Duration::from_secs(60);
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.scheduled_tasks.insert(
                "task-up".into(),
                crate::app::agent::ScheduledTaskInfo {
                    task_id: "task-up".into(),
                    prompt: "old prompt".into(),
                    human_schedule: "every 5 minutes".into(),
                    created_at: original_created_at,
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                    tag: "loop".into(),
                    last_subagent_id: Some("sub-abc".into()),
                },
            );
        }

        let notif = make_created_ext_notif(
            "sess-1",
            "task-up",
            "new prompt",
            "every 10 minutes",
            Some("2026-02-02T02:02:02Z"),
        );
        assert!(handle_scheduled_task_created(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.session.scheduled_tasks.len(), 1, "no duplicate chip");
        let info = agent.session.scheduled_tasks.get("task-up").unwrap();
        assert_eq!(info.prompt, "new prompt");
        assert_eq!(info.human_schedule, "every 10 minutes");
        assert_eq!(info.next_fire_at.as_deref(), Some("2026-02-02T02:02:02Z"));
        assert_eq!(
            info.created_at, original_created_at,
            "chip identity (countdown anchor) preserved"
        );
        assert_eq!(
            info.last_subagent_id.as_deref(),
            Some("sub-abc"),
            "click-through linkage preserved across an update"
        );
    }

    #[test]
    fn created_updates_correct_agent_when_active_view_differs() {
        let mut app = make_app_two_agents();
        let notif = make_created_ext_notif(
            "sess-owner",
            "task-new",
            "check PR",
            "every 5m",
            Some("2026-06-01T12:00:00Z"),
        );
        let needs_redraw = handle_scheduled_task_created(&notif, &mut app);
        assert!(
            !needs_redraw,
            "non-active agent mutation should not trigger redraw"
        );

        let agent0 = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent0.session.scheduled_tasks.contains_key("task-new"),
            "task must be created on the owning agent"
        );

        let agent1 = app.agents.get(&AgentId(1)).unwrap();
        assert!(
            agent1.session.scheduled_tasks.is_empty(),
            "non-owning agent must not receive the task"
        );
    }

    #[test]
    fn deleted_removes_from_correct_agent_when_active_view_differs() {
        let mut app = make_app_two_agents();
        // Seed task on agent 0.
        {
            let agent0 = app.agents.get_mut(&AgentId(0)).unwrap();
            agent0.session.scheduled_tasks.insert(
                "task-rm".into(),
                crate::app::agent::ScheduledTaskInfo {
                    task_id: "task-rm".into(),
                    prompt: "check PR".into(),
                    human_schedule: "every 5m".into(),
                    created_at: Instant::now(),
                    next_fire_at: Some("2026-01-01T00:00:00Z".into()),
                    tag: "loop".into(),
                    last_subagent_id: None,
                },
            );
        }

        let notif = make_deleted_ext_notif("sess-owner", "task-rm");
        let needs_redraw = handle_scheduled_task_deleted(&notif, &mut app);
        assert!(
            !needs_redraw,
            "non-active agent mutation should not trigger redraw"
        );

        let agent0 = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent0.session.scheduled_tasks.is_empty(),
            "task must be removed from the owning agent"
        );
    }

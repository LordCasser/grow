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
                crate::app::session::ScheduledTaskInfo {
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
                crate::app::session::ScheduledTaskInfo {
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
                crate::app::session::ScheduledTaskInfo {
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
                crate::app::session::ScheduledTaskInfo {
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
                crate::app::session::ScheduledTaskInfo {
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

#[test]
fn older_snapshot_merges_untouched_tasks_without_rolling_back_live_fire() {
    let mut app = make_app_with_agent("sess-1");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .scheduled_tasks
        .insert(
            "task-a".into(),
            crate::app::session::ScheduledTaskInfo {
                task_id: "task-a".into(),
                prompt: "old replay prompt".into(),
                human_schedule: "every minute".into(),
                created_at: Instant::now(),
                next_fire_at: Some("old-fire".into()),
                tag: "loop".into(),
                last_subagent_id: None,
            },
        );
    let fired = make_versioned_fired_notif(
        "sess-1",
        "task-a",
        Some("new-fire"),
        "child-a",
        "00000000-0000-0000-0000-000000000001",
        11,
    );
    assert!(handle_scheduled_task_fired(&fired, &mut app));

    apply_scheduler_load_snapshot(
        &mut app,
        &acp::SessionId::new("sess-1"),
        shell::extensions::notification::SchedulerLoadSnapshot {
            generation: "00000000-0000-0000-0000-000000000001".into(),
            revision: 10,
            tasks: vec![
                shell::extensions::notification::ScheduledTaskLoadSnapshot {
                    task_id: "task-a".into(),
                    prompt: "snapshot prompt".into(),
                    human_schedule: "every 2 minutes".into(),
                    next_fire_at: Some("stale-fire".into()),
                    last_subagent_id: Some("stale-child".into()),
                },
                shell::extensions::notification::ScheduledTaskLoadSnapshot {
                    task_id: "task-b".into(),
                    prompt: "task b".into(),
                    human_schedule: "every 5 minutes".into(),
                    next_fire_at: Some("b-fire".into()),
                    last_subagent_id: Some("child-b".into()),
                },
            ],
        },
    );

    let agent = app.agents.get(&AgentId(0)).unwrap();
    let a = agent.session.scheduled_tasks.get("task-a").unwrap();
    assert_eq!(a.next_fire_at.as_deref(), Some("new-fire"));
    assert_eq!(a.last_subagent_id.as_deref(), Some("child-a"));
    let b = agent.session.scheduled_tasks.get("task-b").unwrap();
    assert_eq!(b.next_fire_at.as_deref(), Some("b-fire"));
    assert_eq!(b.last_subagent_id.as_deref(), Some("child-b"));
}

#[test]
fn older_snapshot_does_not_resurrect_deleted_or_replayed_tasks() {
    let mut app = make_app_with_agent("sess-1");
    {
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        for task_id in ["task-deleted", "replay-only"] {
            agent.session.scheduled_tasks.insert(
                task_id.into(),
                crate::app::session::ScheduledTaskInfo {
                    task_id: task_id.into(),
                    prompt: "replayed".into(),
                    human_schedule: "every minute".into(),
                    created_at: Instant::now(),
                    next_fire_at: Some("old-fire".into()),
                    tag: "loop".into(),
                    last_subagent_id: None,
                },
            );
        }
    }
    let deleted = SessionNotification {
        session_id: acp::SessionId::new("sess-1"),
        update: GrowSessionUpdate::ScheduledTaskDeleted {
            task_id: "task-deleted".into(),
        },
        meta: Some(serde_json::json!({
            "grow/schedulerGeneration": "00000000-0000-0000-0000-000000000001",
            "grow/schedulerRevision": 11,
        })),
    };
    let raw = serde_json::value::to_raw_value(&deleted).unwrap();
    let deleted = acp::ExtNotification::new(
        "grow/scheduled_task_deleted",
        std::sync::Arc::from(raw),
    );
    assert!(handle_scheduled_task_deleted(&deleted, &mut app));

    apply_scheduler_load_snapshot(
        &mut app,
        &acp::SessionId::new("sess-1"),
        shell::extensions::notification::SchedulerLoadSnapshot {
            generation: "00000000-0000-0000-0000-000000000001".into(),
            revision: 10,
            tasks: vec![shell::extensions::notification::ScheduledTaskLoadSnapshot {
                task_id: "task-deleted".into(),
                prompt: "stale snapshot".into(),
                human_schedule: "every minute".into(),
                next_fire_at: Some("stale-fire".into()),
                last_subagent_id: None,
            }],
        },
    );

    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(!agent.session.scheduled_tasks.contains_key("task-deleted"));
    assert!(!agent.session.scheduled_tasks.contains_key("replay-only"));
}

#[test]
fn live_task_versions_do_not_block_older_notifications_for_other_tasks() {
    let mut app = make_app_with_agent("sess-1");
    apply_scheduler_load_snapshot(
        &mut app,
        &acp::SessionId::new("sess-1"),
        shell::extensions::notification::SchedulerLoadSnapshot {
            generation: "00000000-0000-0000-0000-000000000001".into(),
            revision: 10,
            tasks: vec![
                shell::extensions::notification::ScheduledTaskLoadSnapshot {
                    task_id: "task-a".into(),
                    prompt: "snapshot a".into(),
                    human_schedule: "every minute".into(),
                    next_fire_at: Some("a-before".into()),
                    last_subagent_id: None,
                },
                shell::extensions::notification::ScheduledTaskLoadSnapshot {
                    task_id: "task-b".into(),
                    prompt: "snapshot b".into(),
                    human_schedule: "every 5 minutes".into(),
                    next_fire_at: Some("b-before".into()),
                    last_subagent_id: None,
                },
            ],
        },
    );

    let fired_a = make_versioned_fired_notif(
        "sess-1",
        "task-a",
        Some("a-r12"),
        "child-a",
        "00000000-0000-0000-0000-000000000001",
        12,
    );
    assert!(handle_scheduled_task_fired(&fired_a, &mut app));

    let created_b = SessionNotification {
        session_id: acp::SessionId::new("sess-1"),
        update: GrowSessionUpdate::ScheduledTaskCreated {
            task_id: "task-b".into(),
            prompt: "live b".into(),
            human_schedule: "every 10 minutes".into(),
            next_fire_at: Some("b-r11".into()),
        },
        meta: Some(serde_json::json!({
            "grow/schedulerGeneration": "00000000-0000-0000-0000-000000000001",
            "grow/schedulerRevision": 11,
        })),
    };
    let raw = serde_json::value::to_raw_value(&created_b).unwrap();
    let created_b = acp::ExtNotification::new("grow/scheduled_task_created", std::sync::Arc::from(raw));
    assert!(handle_scheduled_task_created(&created_b, &mut app));

    let agent = app.agents.get(&AgentId(0)).unwrap();
    let a = agent.session.scheduled_tasks.get("task-a").unwrap();
    assert_eq!(a.next_fire_at.as_deref(), Some("a-r12"));
    assert_eq!(a.last_subagent_id.as_deref(), Some("child-a"));
    let b = agent.session.scheduled_tasks.get("task-b").unwrap();
    assert_eq!(b.prompt, "live b");
    assert_eq!(b.next_fire_at.as_deref(), Some("b-r11"));
}

#[test]
fn snapshot_epoch_fences_old_live_events_and_absent_fire_stays_absent() {
    let mut app = make_app_with_agent("sess-1");
    apply_scheduler_load_snapshot(
        &mut app,
        &acp::SessionId::new("sess-1"),
        shell::extensions::notification::SchedulerLoadSnapshot {
            generation: "00000000-0000-0000-0000-000000000002".into(),
            revision: 10,
            tasks: Vec::new(),
        },
    );

    let late_old_event = make_versioned_fired_notif(
        "sess-1",
        "old-task",
        Some("old-next-fire"),
        "old-child",
        "00000000-0000-0000-0000-000000000001",
        99,
    );
    assert!(!handle_scheduled_task_fired(&late_old_event, &mut app));
    assert!(app.agents[&AgentId(0)].session.scheduled_tasks.is_empty());

    let absent_one_shot = make_versioned_fired_notif(
        "sess-1",
        "already-absent",
        None,
        "child",
        "00000000-0000-0000-0000-000000000002",
        11,
    );
    assert!(handle_scheduled_task_fired(&absent_one_shot, &mut app));
    assert!(app.agents[&AgentId(0)].session.scheduled_tasks.is_empty());
}

#[test]
fn newer_live_generation_fences_old_epoch_before_its_snapshot() {
    let mut app = make_app_with_agent("sess-1");
    apply_scheduler_load_snapshot(
        &mut app,
        &acp::SessionId::new("sess-1"),
        shell::extensions::notification::SchedulerLoadSnapshot {
            generation: "00000000-0000-0000-0000-000000000001".into(),
            revision: 10,
            tasks: Vec::new(),
        },
    );

    let new_epoch_created = SessionNotification {
        session_id: acp::SessionId::new("sess-1"),
        update: GrowSessionUpdate::ScheduledTaskCreated {
            task_id: "new-epoch-task".into(),
            prompt: "new epoch".into(),
            human_schedule: "every minute".into(),
            next_fire_at: Some("new-epoch-fire".into()),
        },
        meta: Some(serde_json::json!({
            "grow/schedulerGeneration": "00000000-0000-0000-0000-000000000002",
            "grow/schedulerRevision": 1,
        })),
    };
    let raw = serde_json::value::to_raw_value(&new_epoch_created).unwrap();
    let new_epoch_created = acp::ExtNotification::new(
        "grow/scheduled_task_created",
        std::sync::Arc::from(raw),
    );
    assert!(handle_scheduled_task_created(&new_epoch_created, &mut app));

    let late_old_event = make_versioned_fired_notif(
        "sess-1",
        "old-epoch-task",
        Some("stale-fire"),
        "stale-child",
        "00000000-0000-0000-0000-000000000001",
        99,
    );
    assert!(!handle_scheduled_task_fired(&late_old_event, &mut app));
    let tasks = &app.agents[&AgentId(0)].session.scheduled_tasks;
    assert_eq!(tasks.len(), 1);
    assert!(tasks.contains_key("new-epoch-task"));
}

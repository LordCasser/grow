#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn nested_subagent_lifecycle_registers_flat_descendant_route() {
        let mut app = make_app_with_agent("root-session");
        handle(
            make_ext_session_notification(
                "root-session",
                test_subagent_spawned("root-session", "child-session"),
            ),
            &mut app,
        );
        handle(
            make_ext_session_notification(
                "child-session",
                test_subagent_spawned("child-session", "grandchild-session"),
            ),
            &mut app,
        );

        let root = &app.agents[&AgentId(0)];
        assert!(root.session.subagent_sessions.contains_key("grandchild-session"));
        assert!(root.subagent_views.contains_key("grandchild-session"));
        assert!(matches!(
            find_session_match(&app, &acp::SessionId::new("grandchild-session")),
            Some(SessionMatch::Child(AgentId(0)))
        ));

        handle(
            make_ext_session_notification(
                "child-session",
                GrowSessionUpdate::SubagentFinished {
                    subagent_id: "grandchild-session".into(),
                    child_session_id: "grandchild-session".into(),
                    status: "completed".into(),
                    error: None,
                    tool_calls: 1,
                    turns: 1,
                    duration_ms: 10,
                    tokens_used: 1,
                    output: None,
                },
            ),
            &mut app,
        );
        assert!(app.agents[&AgentId(0)].session.subagent_sessions["grandchild-session"].finished);
    }

    #[test]
    fn child_model_and_agent_updates_refresh_parent_task_projection() {
        let mut app = make_app_with_agent("root-session");
        handle(
            make_ext_session_notification(
                "root-session",
                test_subagent_spawned("root-session", "child-session"),
            ),
            &mut app,
        );
        {
            let child = app.agents.get_mut(&AgentId(0)).unwrap().subagent_views
                .get_mut("child-session")
                .unwrap();
            let model_id = shell::agent::models::ModelId::new("grow-child");
            child.session.models.available.insert(
                model_id.clone(),
                shell::agent::models::ModelInfo::new(model_id, "Grow Child"),
            );
        }

        handle(
            make_ext_session_notification(
                "child-session",
                GrowSessionUpdate::ModelChanged {
                    model_id: "grow-child".into(),
                    reasoning_effort: None,
                },
            ),
            &mut app,
        );
        handle(
            make_ext_session_notification(
                "child-session",
                GrowSessionUpdate::AgentChanged {
                    agent_name: "reviewer".into(),
                },
            ),
            &mut app,
        );

        let info = &app.agents[&AgentId(0)].session.subagent_sessions["child-session"];
        assert_eq!(info.model.as_deref(), Some("grow-child"));
        assert_eq!(info.subagent_type.as_ref(), "reviewer");
    }

    #[test]
    fn replayed_spawn_preserves_rearmed_child_control_identity() {
        let mut app = make_app_with_agent("root-session");
        let spawn = || {
            make_ext_session_notification(
                "root-session",
                test_subagent_spawned("root-session", "child-session"),
            )
        };
        handle(spawn(), &mut app);
        {
            let root = app.agents.get_mut(&AgentId(0)).unwrap();
            root.subagent_views
                .get_mut("child-session")
                .unwrap()
                .session
                .begin_model_switch_for_test();
            root.begin_session_reload(7);
        }
        let expected = app.agents[&AgentId(0)].subagent_views["child-session"]
            .session
            .current_control_token_for_test();
        assert_eq!(
            expected.generation, 0,
            "transport reconnect must preserve the semantic user-intent generation"
        );
        assert!(
            expected.dispatch_generation > 0,
            "reload must rearm the child transport dispatch epoch"
        );

        handle(spawn(), &mut app);

        let actual = app.agents[&AgentId(0)].subagent_views["child-session"]
            .session
            .current_control_token_for_test();
        assert_eq!(
            actual, expected,
            "durable spawn replay must reuse the exact child view instead of resetting its control revision"
        );
    }

    #[test]
    fn subagent_permission_decision_renders_in_parent_scrollback_only() {
        let mut app = make_app_with_agent("root-session");
        handle(
            make_ext_session_notification(
                "root-session",
                test_subagent_spawned("root-session", "019ff8d7-child"),
            ),
            &mut app,
        );
        handle(
            make_ext_session_notification(
                "root-session",
                GrowSessionUpdate::SubagentPermissionDecision {
                    child_session_id: "019ff8d7-child".into(),
                    subagent_type: Some("software-coder".into()),
                    description: Some("run tests".into()),
                    tool_call_id: "tool-7".into(),
                    tool_name: "run_terminal_command".into(),
                    access_kind: "bash".into(),
                    access_summary: Some("cargo test -p shell".into()),
                    access_detail: Some("cargo test -p shell -- --nocapture".into()),
                    outcome: shell::extensions::notification::SubagentPermissionOutcome::Unavailable,
                    source: "main_agent".into(),
                    reason: Some("main-agent judgment timed out".into()),
                    classifier_reason: Some("provider did not respond".into()),
                    latency_ms: Some(30_000),
                },
            ),
            &mut app,
        );

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let first = agent
            .scrollback
            .entries_mut()
            .find_map(|entry| match &entry.block {
                crate::scrollback::block::RenderBlock::SubagentPermission(block) => block.member(0),
                _ => None,
            })
            .expect("structured permission audit block");
        assert!(first.compact_text().contains(
            "Subagent permission · Explore scan src/ · unavailable → denied"
        ));
        assert!(
            !first.compact_text().contains("019ff8d7"),
            "compact identity must match the Subagents pane instead of exposing a session id"
        );
        assert!(
            first
                .compact_text()
                .contains("run_terminal_command [bash: cargo test -p shell]")
        );
        assert!(
            first
                .detail_text()
                .contains("Reason: main-agent judgment timed out")
        );
        for (outcome, expected) in [
            (
                shell::extensions::notification::SubagentPermissionOutcome::Approved,
                "approved",
            ),
            (
                shell::extensions::notification::SubagentPermissionOutcome::Denied,
                "denied",
            ),
            (
                shell::extensions::notification::SubagentPermissionOutcome::TimedOut,
                "timed out → denied",
            ),
        ] {
            handle(
                make_ext_session_notification(
                    "root-session",
                    GrowSessionUpdate::SubagentPermissionDecision {
                        child_session_id: "019ff8d7-child".into(),
                        subagent_type: Some("software-coder".into()),
                        description: None,
                        tool_call_id: format!("tool-{expected}"),
                        tool_name: "run_terminal_command".into(),
                        access_kind: "bash".into(),
                        access_summary: None,
                        access_detail: None,
                        outcome,
                        source: "main_agent".into(),
                        reason: None,
                        classifier_reason: None,
                        latency_ms: Some(1),
                    },
                ),
                &mut app,
            );
        }
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let labels = agent
            .scrollback
            .entries_mut()
            .filter_map(|entry| match &entry.block {
                crate::scrollback::block::RenderBlock::SubagentPermission(block) => Some(
                    block
                        .members()
                        .iter()
                        .map(|member| member.outcome_label())
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .flatten()
            .collect::<Vec<_>>();
        assert!(labels.contains(&"approved"));
        assert!(labels.contains(&"denied"));
        assert!(labels.contains(&"timed out → denied"));
        assert_eq!(
            labels.len(),
            4,
            "each audit update must project as exactly one structured UI block"
        );
    }

    /// Fresh root restore discovers the direct child's durable transcript via
    /// the parent's canonical subagent spawn fact.
    /// The persisted eventId seeds the immediate-parent highwater, so the same
    /// lifecycle event flushed from the leader's ancestor-load live buffer is
    /// not projected a second time.
    #[test]
    fn descendant_restore_uses_owned_disk_layout_and_dedups_live_overlap() {
        with_replay_disk_home(|home| {
            let root_sid = "restore-root-owned";
            let child_sid = "restore-child-owned";
            let grandchild_sid = "restore-grandchild-owned";
            write_subagent_spawn_timeline(home, root_sid, child_sid, "restore child");

            let mut nested = SessionNotification {
                session_id: acp::SessionId::new(child_sid),
                update: test_subagent_spawned(child_sid, grandchild_sid),
                meta: Some(serde_json::json!({ "eventId": format!("{child_sid}-10") })),
            };
            let persisted = serde_json::json!({
                "timestamp": 1,
                "method": "_grow/session/update",
                "params": serde_json::to_value(&nested).unwrap(),
            });
            let control_line = |event_id: &str, update: GrowSessionUpdate| {
                let notification = SessionNotification {
                    session_id: acp::SessionId::new(child_sid),
                    update,
                    meta: Some(serde_json::json!({ "eventId": event_id })),
                };
                serde_json::json!({
                    "timestamp": 1,
                    "method": "_grow/session/update",
                    "params": serde_json::to_value(notification).unwrap(),
                })
                .to_string()
            };
            write_child_updates_jsonl(
                home,
                child_sid,
                &[
                    persisted.to_string(),
                    control_line(
                        &format!("{child_sid}-11"),
                        GrowSessionUpdate::ModelChanged {
                            model_id: "restored-model".into(),
                            reasoning_effort: Some("high".into()),
                        },
                    ),
                    control_line(
                        &format!("{child_sid}-12"),
                        GrowSessionUpdate::AgentChanged {
                            agent_name: "quality:reviewer".into(),
                        },
                    ),
                ]
                .join("\n"),
            );

            let mut app = make_app_with_agent(root_sid);
            handle(
                make_ext_session_notification(
                    root_sid,
                    test_subagent_spawned(root_sid, child_sid),
                ),
                &mut app,
            );
            {
                let child = app.agents.get_mut(&AgentId(0)).unwrap().subagent_views
                    .get_mut(child_sid)
                    .unwrap();
                let model = shell::agent::models::ModelId::new("restored-model");
                child.session.models.available.insert(
                    model.clone(),
                    shell::agent::models::ModelInfo::new(model, "Restored Model"),
                );
            }
            let before_restore = app.agents[&AgentId(0)].scrollback.len();

            crate::app::subagent::restore_descendant_state(&mut app, AgentId(0));

            let root = &app.agents[&AgentId(0)];
            assert!(root.session.subagent_sessions.contains_key(grandchild_sid));
            assert!(root.subagent_views.contains_key(grandchild_sid));
            assert_eq!(root.scrollback.len(), before_restore + 1);
            assert_eq!(
                root.subagent_views[child_sid].session.last_applied_grow_event_seq,
                Some(12),
                "the persisted child eventId must seed the child-source highwater"
            );
            assert_eq!(
                root.subagent_views[child_sid].session.agent_name(),
                Some("quality:reviewer")
            );
            assert_eq!(
                root.subagent_views[child_sid].session.models.current.as_ref().map(|id| id.0.as_ref()),
                Some("restored-model")
            );
            assert!(matches!(
                find_session_match(&app, &acp::SessionId::new(grandchild_sid)),
                Some(SessionMatch::Child(AgentId(0)))
            ));

            let after_restore = root.scrollback.len();
            nested.meta = Some(serde_json::json!({ "eventId": format!("{child_sid}-10") }));
            let raw = serde_json::value::to_raw_value(&nested).unwrap();
            let live = acp::ExtNotification::new(
                "grow/session_notification",
                std::sync::Arc::from(raw),
            );
            handle_ext_notification(&live, &mut app);
            assert_eq!(
                app.agents[&AgentId(0)].scrollback.len(),
                after_restore,
                "the replay/live overlap must project the nested spawn exactly once"
            );
        });
    }

    #[test]
    fn child_view_uses_independent_permission_mode_from_spawn_event() {
        let mut app = make_app_with_agent("sess-1");
        let mut update = test_subagent_spawned("sess-1", "child-auto");
        let GrowSessionUpdate::SubagentSpawned {
            permission_mode,
            effective_permission_mode,
            ..
        } = &mut update
        else {
            unreachable!("test helper must return SubagentSpawned");
        };
        *permission_mode = Some("auto".into());
        *effective_permission_mode = Some("auto".into());

        handle(
            make_ext_session_notification("sess-1", update),
            &mut app,
        );

        let child = app.agents[&AgentId(0)]
            .subagent_views
            .get("child-auto")
            .expect("child view created");
        assert!(child.session.is_auto());
        assert!(!child.session.is_always_approve());
    }

    /// On resume, a replayed spawn+finish pair leaves the subagent terminal.
    #[test]
    fn replayed_subagent_finished_marks_orphan_terminal() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let spawned = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_spawned",
                "subagent_id": "sa-1",
                "parent_session_id": "sess-1",
                "child_session_id": "child-1",
                "subagent_type": "general-purpose",
                "description": "orphan review",
            }),
            "sess-1-1",
        );
        handle_ext_notification(&spawned, &mut app);

        let finished = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_finished",
                "subagent_id": "sa-1",
                "child_session_id": "child-1",
                "status": "cancelled",
                "error": "interrupted by process restart",
                "tool_calls": 0,
                "turns": 0,
                "duration_ms": 1000,
                "tokens_used": 0,
            }),
            "sess-1-2",
        );
        handle_ext_notification(&finished, &mut app);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent
            .session.subagent_sessions
            .get("child-1")
            .expect("subagent present after replay");
        assert!(
            info.finished,
            "orphan must be terminal after replayed subagent_finished"
        );
        assert_eq!(info.status.as_deref(), Some("cancelled"));
    }

    #[test]
    fn replaying_the_same_subagent_lifecycle_twice_keeps_one_started_and_one_terminal_row() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let spawned = || subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_spawned",
                "subagent_id": "sa-once",
                "parent_session_id": "sess-1",
                "child_session_id": "child-once",
                "subagent_type": "reviewer",
                "description": "review once",
            }),
            "subagent-start-once",
        );
        let finished = || subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_finished",
                "subagent_id": "sa-once",
                "child_session_id": "child-once",
                "status": "completed",
                "error": null,
                "tool_calls": 1,
                "turns": 1,
                "duration_ms": 250,
                "tokens_used": 10,
            }),
            "subagent-finish-once",
        );

        for _ in 0..2 {
            handle_ext_notification(&spawned(), &mut app);
            handle_ext_notification(&finished(), &mut app);
        }

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(agent.scrollback.len(), 2);
        assert!(
            agent
                .session
                .subagent_sessions
                .get("child-once")
                .is_some_and(|info| info.finished)
        );
    }

    #[test]
    fn replayed_spawn_does_not_reset_existing_terminal_entity() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let spawn = || {
            subagent_ext_replay(
                "sess-1",
                serde_json::json!({
                    "sessionUpdate": "subagent_spawned",
                    "subagent_id": "sa-preserve",
                    "parent_session_id": "sess-1",
                    "child_session_id": "child-preserve",
                    "subagent_type": "reviewer",
                    "description": "preserve terminal state",
                }),
                "spawn-preserve",
            )
        };
        handle_ext_notification(&spawn(), &mut app);
        {
            let info = app.agents[&AgentId(0)]
                .session
                .subagent_sessions
                .get_mut("child-preserve")
                .unwrap();
            info.finished = true;
            info.status = Some("completed".into());
            info.tokens_used = Some(91);
            info.duration_ms = Some(1200);
        }

        // The same durable spawn fact is replayed again. It must merge its
        // descriptor without turning the already-terminal entity running.
        handle_ext_notification(&spawn(), &mut app);
        let info = app.agents[&AgentId(0)]
            .session
            .subagent_sessions
            .get("child-preserve")
            .unwrap();
        assert!(info.finished);
        assert_eq!(info.status.as_deref(), Some("completed"));
        assert_eq!(info.tokens_used, Some(91));
        assert_eq!(info.duration_ms, Some(1200));
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 1);
    }

    #[test]
    fn finished_before_replayed_spawn_does_not_append_reversed_started_row() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let finished = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_finished",
                "subagent_id": "sa-order",
                "child_session_id": "child-order",
                "status": "completed",
                "error": null,
                "tool_calls": 1,
                "turns": 1,
                "duration_ms": 250,
                "tokens_used": 10,
            }),
            "finish-order",
        );
        assert!(handle_ext_notification(&finished, &mut app));

        let spawned = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_spawned",
                "subagent_id": "sa-order",
                "parent_session_id": "sess-1",
                "child_session_id": "child-order",
                "subagent_type": "reviewer",
                "description": "ordered lifecycle",
            }),
            "spawn-order",
        );
        assert!(handle_ext_notification(&spawned, &mut app));

        let agent = &app.agents[&AgentId(0)];
        assert_eq!(agent.scrollback.len(), 1);
        assert!(agent
            .session
            .subagent_sessions
            .get("child-order")
            .is_some_and(|info| info.finished));
        let (_, entry) = agent.scrollback.iter_entries().next().unwrap();
        let crate::scrollback::block::RenderBlock::Subagent(block) = &entry.block else {
            panic!("out-of-order lifecycle must leave a subagent row");
        };
        assert!(!block.is_running(), "a terminal row must not be followed by Started");
    }

    /// `cancelled = false` must finalize the row, not revert "killing" to "running".
    #[test]
    fn kill_finalizes_orphan_when_shell_reports_not_cancelled() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let spawned = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_spawned",
                "subagent_id": "sa-1",
                "parent_session_id": "sess-1",
                "child_session_id": "child-1",
                "subagent_type": "general-purpose",
                "description": "orphan review",
            }),
            "sess-1-1",
        );
        handle_ext_notification(&spawned, &mut app);

        // User clicks kill after load.
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.loading_replay = false;
            let info = agent.session.subagent_sessions.get_mut("child-1").unwrap();
            assert!(!info.finished);
            info.pending_kill = true;
            info.kill_requested_at = Some(std::time::Instant::now());
        }

        // Shell: cancelled=false (nothing live), no real status → "cancelled".
        let finalized = finalize_killed_subagent(
            &mut app,
            &acp::SessionId::new("sess-1".to_owned()),
            "sa-1",
            "cancelled",
        );
        assert!(finalized, "row should have been finalized");

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.subagent_sessions.get("child-1").unwrap();
        assert!(info.finished, "kill must finalize the stuck orphan row");
        assert_eq!(info.status.as_deref(), Some("cancelled"));
        assert!(
            !info.pending_kill,
            "pending_kill must clear so it can't revert"
        );
        assert!(info.kill_requested_at.is_none());
    }

    /// An already-finished subagent killed → finalize stamps the REAL terminal
    /// status (e.g. "completed"), not a forced "cancelled".
    #[test]
    fn kill_finalizes_orphan_with_real_status_when_already_finished() {
        let mut app = make_app_with_agent("sess-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;

        let spawned = subagent_ext_replay(
            "sess-1",
            serde_json::json!({
                "sessionUpdate": "subagent_spawned",
                "subagent_id": "sa-1",
                "parent_session_id": "sess-1",
                "child_session_id": "child-1",
                "subagent_type": "general-purpose",
                "description": "orphan review",
            }),
            "sess-1-1",
        );
        handle_ext_notification(&spawned, &mut app);
        {
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.session.loading_replay = false;
            let info = agent.session.subagent_sessions.get_mut("child-1").unwrap();
            info.pending_kill = true;
        }

        let finalized = finalize_killed_subagent(
            &mut app,
            &acp::SessionId::new("sess-1".to_owned()),
            "sa-1",
            "completed",
        );
        assert!(finalized, "row should have been finalized");

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.subagent_sessions.get("child-1").unwrap();
        assert!(info.finished);
        assert_eq!(
            info.status.as_deref(),
            Some("completed"),
            "already-finished kill must stamp the real terminal status"
        );
    }

    /// Regression: replay from `updates.jsonl` emits `grow/session/update` (not
    /// `session_notification`). Subagent lifecycle events must still populate
    /// `subagent_sessions` and the parent scrollback `SubagentBlock`.
    #[test]
    fn ext_session_update_replay_handles_subagent_spawned_and_finished() {
        let mut app = make_app_with_agent("sess-parent");
        let child_sid = "child-sess-replay";

        let affected = handle(
            make_ext_session_notification_with_method(
                "sess-parent",
                "grow/session/update",
                test_subagent_spawned("sess-parent", child_sid),
            ),
            &mut app,
        );
        assert!(
            affected,
            "SubagentSpawned on the active agent must request a redraw"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent
            .session.subagent_sessions
            .get(child_sid)
            .expect("SubagentSpawned must register subagent_sessions");
        assert_eq!(info.description.as_ref(), "scan src/");
        assert_eq!(info.subagent_type.as_ref(), "explore");
        assert!(
            agent.subagent_views.contains_key(child_sid),
            "SubagentSpawned must create subagent_views eagerly"
        );
        let entry_id = info
            .scrollback_entry_id
            .expect("spawn must stash scrollback_entry_id on SubagentInfo");
        assert_eq!(agent.scrollback.len(), 1);
        let entry = agent.scrollback.get_by_id(entry_id).unwrap();
        let RenderBlock::Subagent(sb) = &entry.block else {
            panic!("SubagentSpawned must push a SubagentBlock to parent scrollback");
        };
        assert_eq!(sb.child_session_id, child_sid);
        assert!(matches!(sb.kind, SubagentBlockKind::Started));
        assert!(
            !agent.scrollback.needs_animation(),
            "started scrollback events are immutable; live activity belongs to the entity projection"
        );

        let affected = handle(
            make_ext_session_notification_with_method(
                "sess-parent",
                "grow/session/update",
                test_subagent_finished(child_sid),
            ),
            &mut app,
        );
        assert!(
            affected,
            "SubagentFinished on the active agent must request a redraw"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.subagent_sessions.get(child_sid).unwrap();
        assert!(info.finished);
        assert_eq!(info.status.as_deref(), Some("completed"));
        assert_eq!(info.tool_calls, Some(2));
        assert_eq!(info.turns, Some(1));
        assert_eq!(info.duration_ms, Some(500));
        assert_eq!(info.scrollback_entry_id, Some(entry_id));

        assert_eq!(agent.scrollback.len(), 2);
        let started = agent.scrollback.get_by_id(entry_id).unwrap();
        let RenderBlock::Subagent(started_block) = &started.block else {
            panic!("started subagent event must remain a Subagent block");
        };
        assert!(matches!(started_block.kind, SubagentBlockKind::Started));
        assert!(!started.is_running, "finish_running must stop the started event");
        let terminal = agent.scrollback.entry(1).unwrap();
        let RenderBlock::Subagent(terminal_block) = &terminal.block else {
            panic!("finished subagent must append a terminal Subagent block");
        };
        match &terminal_block.kind {
            SubagentBlockKind::Completed { elapsed } => {
                assert_eq!(*elapsed, std::time::Duration::from_millis(500));
            }
            other => {
                panic!("blocking subagent must append Completed, got {other:?}")
            }
        }
        assert!(
            !agent.scrollback.needs_animation(),
            "finished subagent events must not keep scrollback animation"
        );
    }

    #[test]
    fn workflow_children_do_not_emit_orphan_subagent_lifecycle_rows() {
        let mut app = make_app_with_agent("sess-parent");
        let child_sid = "workflow-child";
        let mut spawned = test_subagent_spawned("sess-parent", child_sid);
        let GrowSessionUpdate::SubagentSpawned {
            workflow_run_id, ..
        } = &mut spawned
        else {
            unreachable!("test fixture is SubagentSpawned")
        };
        *workflow_run_id = Some("workflow-run-1".into());

        assert!(handle(
            make_ext_session_notification("sess-parent", spawned),
            &mut app,
        ));
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 0);

        assert!(handle(
            make_ext_session_notification(
                "sess-parent",
                test_subagent_finished(child_sid),
            ),
            &mut app,
        ));
        let agent = &app.agents[&AgentId(0)];
        assert!(agent.session.subagent_sessions[child_sid].finished);
        assert_eq!(
            agent.scrollback.len(),
            0,
            "workflow-owned children use the workflow projection, so Finished must not create an orphan row"
        );
    }

    /// Live activity belongs only to the mutable entity projection used by the
    /// tasks/dashboard panes. Started/Finished scrollback rows remain immutable.
    #[test]
    fn subagent_activity_label_stamps_info_and_clears_on_finish() {
        let mut app = make_app_with_agent("sess-parent");
        let child_sid = "child-activity";
        let _ = handle(
            make_ext_session_notification(
                "sess-parent",
                test_subagent_spawned("sess-parent", child_sid),
            ),
            &mut app,
        );

        // A live child message chunk resolves "Responding" in entity state.
        let _ = handle(
            make_agent_chunk_with_event(child_sid, "child text", "p-child", None),
            &mut app,
        );
        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.subagent_sessions.get(child_sid).unwrap();
        assert_eq!(info.activity_label.as_deref(), Some("Responding"));
        let entry_id = info.scrollback_entry_id.unwrap();

        // SubagentProgress recomputes from the child tracker and restamps.
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session.subagent_sessions
            .get_mut(child_sid)
            .unwrap()
            .activity_label = None;
        let _ = handle(
            make_ext_session_notification(
                "sess-parent",
                test_subagent_progress("sess-parent", child_sid),
            ),
            &mut app,
        );
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session.subagent_sessions
                .get(child_sid)
                .unwrap()
                .activity_label
                .as_deref(),
            Some("Responding")
        );

        let _ = handle(
            make_ext_session_notification("sess-parent", test_subagent_finished(child_sid)),
            &mut app,
        );
        let agent = app.agents.get(&AgentId(0)).unwrap();
        let info = agent.session.subagent_sessions.get(child_sid).unwrap();
        assert!(
            info.activity_label.is_none(),
            "finish must clear the info label"
        );
        let entry = agent.scrollback.get_by_id(entry_id).unwrap();
        let RenderBlock::Subagent(sb) = &entry.block else {
            panic!("expected immutable SubagentStarted block");
        };
        assert!(matches!(sb.kind, SubagentBlockKind::Started));
    }

    /// Regression: replayed SubagentSpawned (resumed_from unset) must load child
    /// updates.jsonl so fullscreen scrollback is not prompt-only.
    #[test]
    fn subagent_spawned_replays_child_updates_without_resumed_from() {
        with_replay_disk_home(|home| {
            let child_sid = "child-with-updates";
            let mut app = make_app_with_agent("sess-parent");
            spawn_subagent_with_optional_updates(
                home,
                &mut app,
                child_sid,
                Some(&(child_tool_line(child_sid) + "\n")),
            );

            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                1,
                "spawn must replay exactly one tool call"
            );
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| i.child_updates_replayed),
                "spawn must set child_updates_replayed"
            );
        });
    }

    /// Resume: a `SubagentSpawned` during `loading_replay` must defer the child
    /// transcript load (the dominant large-session resume cost) to first open.
    #[test]
    fn subagent_spawned_during_resume_defers_child_replay_until_open() {
        with_replay_disk_home(|home| {
            let child_sid = "child-resume-defer";
            let mut app = make_app_with_agent("sess-parent");
            // Simulate resume: the parent agent is replaying its own session.
            app.agents
                .get_mut(&AgentId(0))
                .unwrap()
                .session
                .loading_replay = true;

            spawn_subagent_with_optional_updates(
                home,
                &mut app,
                child_sid,
                Some(&(child_tool_line(child_sid) + "\n")),
            );

            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                0,
                "resume spawn must NOT eagerly replay the child transcript"
            );
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| !i.child_updates_replayed),
                "resume spawn must leave child_updates_replayed unset for lazy load"
            );

            // Opening the subagent later triggers the deferred (lazy) replay.
            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.open_subagent_fullscreen(child_sid.to_string());
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                1,
                "opening the subagent after resume must lazily replay its transcript"
            );
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| i.child_updates_replayed),
                "lazy open must set child_updates_replayed"
            );
        });
    }

    /// Regression (resume): a subagent that already finished must still show its
    /// full transcript on open. The finished handler's `TurnCompleted` push is
    /// suppressed during replay — otherwise it vetoes the deferred load
    /// (`subagent_child_needs_replay`), leaving a permanently empty transcript.
    #[test]
    fn subagent_resume_finished_then_open_shows_full_transcript() {
        with_replay_disk_home(|home| {
            let child_sid = "child-resume-finished";
            let mut app = make_app_with_agent("sess-parent");
            app.agents
                .get_mut(&AgentId(0))
                .unwrap()
                .session
                .loading_replay = true;

            spawn_subagent_with_optional_updates(
                home,
                &mut app,
                child_sid,
                Some(&(child_tool_line(child_sid) + "\n")),
            );
            let _ = handle(
                make_ext_session_notification_with_method(
                    "sess-parent",
                    "grow/session/update",
                    test_subagent_finished(child_sid),
                ),
                &mut app,
            );

            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                0,
                "resume must not eagerly load the finished subagent transcript"
            );
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| !i.child_updates_replayed),
                "finished-during-resume must leave child_updates_replayed unset"
            );
            // Even deferred, a finished subagent must not show a running spinner.
            assert!(
                matches!(
                    agent.subagent_views.get(child_sid).unwrap().session.state,
                    AgentState::Idle
                ),
                "finished subagent must be Idle after resume, not TurnRunning"
            );

            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.open_subagent_fullscreen(child_sid.to_string());
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                1,
                "opening a finished subagent after resume must show its transcript"
            );
            // The lazy load reapplies the "Worked for" footer (live parity).
            let child = agent.subagent_views.get(child_sid).unwrap();
            assert!(
                (0..child.scrollback.len()).any(|i| child
                    .scrollback
                    .entry(i)
                    .is_some_and(|e| matches!(e.block, RenderBlock::SessionEvent(_)))),
                "opened finished subagent must show a TurnCompleted footer"
            );
        });
    }

    /// Regression (resume): with a Timeline task prompt AND a persisted child
    /// transcript that echoes that prompt, opening after resume shows the task
    /// exactly once — the deferred open must dedup the replayed prompt echo.
    #[test]
    fn subagent_resume_with_meta_prompt_shows_task_once_after_open() {
        with_replay_disk_home(|home| {
            let parent_sid = "sess-parent";
            let child_sid = "child-resume-meta";
            let task = "scan src/ for auth";
            write_subagent_spawn_timeline(home, parent_sid, child_sid, task);

            let mut app = make_app_with_agent(parent_sid);
            app.agents
                .get_mut(&AgentId(0))
                .unwrap()
                .session
                .loading_replay = true;

            let updates = format!(
                "{}\n{}",
                child_user_message_line(child_sid, task),
                child_tool_line(child_sid)
            );
            spawn_subagent_with_optional_updates(home, &mut app, child_sid, Some(&updates));

            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            agent.open_subagent_fullscreen(child_sid.to_string());
            assert_eq!(
                child_scrollback_matching_prompt_count(agent, child_sid, task),
                1,
                "task prompt must appear exactly once after resume + open"
            );
            assert_eq!(child_scrollback_tool_call_count(agent, child_sid), 1);
        });
    }

    /// Regression: replayed user_message_chunk + Timeline prompt must not duplicate via injection.
    #[test]
    fn subagent_spawn_replay_and_meta_prompt_shows_task_once() {
        with_replay_disk_home(|home| {
            let parent_sid = "sess-parent";
            let child_sid = "child-prompt-once";
            let task = "scan src/ for auth";
            write_subagent_spawn_timeline(home, parent_sid, child_sid, task);

            let mut app = make_app_with_agent(parent_sid);
            let updates = format!(
                "{}\n{}",
                child_user_message_line(child_sid, task),
                child_tool_line(child_sid)
            );
            spawn_subagent_with_optional_updates(home, &mut app, child_sid, Some(&updates));

            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(
                child_scrollback_matching_prompt_count(agent, child_sid, task),
                1,
                "task prompt must appear exactly once in child scrollback"
            );
            assert_eq!(child_scrollback_tool_call_count(agent, child_sid), 1);
        });
    }

    #[test]
    fn subagent_spawn_without_updates_jsonl_is_noop() {
        with_replay_disk_home(|home| {
            let child_sid = "child-no-updates";
            let mut app = make_app_with_agent("sess-parent");
            spawn_subagent_with_optional_updates(home, &mut app, child_sid, None);

            let agent = app.agents.get(&AgentId(0)).unwrap();
            assert_eq!(child_scrollback_tool_call_count(agent, child_sid), 0);
            assert_eq!(
                agent
                    .subagent_views
                    .get(child_sid)
                    .unwrap()
                    .scrollback
                    .len(),
                0
            );
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| i.child_updates_replayed)
            );
        });
    }

    #[test]
    fn subagent_spawn_and_open_replay_is_idempotent() {
        with_replay_disk_home(|home| {
            let child_sid = "child-idempotent";
            let mut app = make_app_with_agent("sess-parent");
            spawn_subagent_with_optional_updates(
                home,
                &mut app,
                child_sid,
                Some(&(child_tool_line(child_sid) + "\n")),
            );

            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            assert_eq!(child_scrollback_tool_call_count(agent, child_sid), 1);
            agent.open_subagent_fullscreen(child_sid.to_string());
            assert_eq!(
                child_scrollback_tool_call_count(agent, child_sid),
                1,
                "open must not duplicate spawn replay when child_updates_replayed is set"
            );
        });
    }

    #[test]
    fn open_subagent_fullscreen_replays_when_flag_false_and_prompt_only() {
        with_replay_disk_home(|home| {
            let child_sid = "child-open-replay";
            let mut app = make_app_with_agent("sess-parent");
            spawn_subagent_with_optional_updates(
                home,
                &mut app,
                child_sid,
                Some(&(child_tool_line(child_sid) + "\n")),
            );

            let agent = app.agents.get_mut(&AgentId(0)).unwrap();
            if let Some(child) = agent.subagent_views.get_mut(child_sid) {
                child.scrollback.clear();
                child
                    .scrollback
                    .push_block(RenderBlock::user_prompt("task only"));
            }
            if let Some(info) = agent.session.subagent_sessions.get_mut(child_sid) {
                info.child_updates_replayed = false;
            }

            agent.open_subagent_fullscreen(child_sid.to_string());

            assert_eq!(child_scrollback_tool_call_count(agent, child_sid), 1);
            assert!(
                agent
                    .session.subagent_sessions
                    .get(child_sid)
                    .is_some_and(|i| i.child_updates_replayed)
            );
        });
    }

    #[test]
    fn ext_session_notification_and_update_equivalent_for_subagent_spawned() {
        let child_sid = "child-equiv";
        let (spawn_notif, finish_notif) =
            run_subagent_lifecycle_via_method("grow/session_notification", child_sid);
        let (spawn_update, finish_update) =
            run_subagent_lifecycle_via_method("grow/session/update", child_sid);

        assert_eq!(spawn_notif.description, spawn_update.description);
        assert_eq!(spawn_notif.subagent_type, spawn_update.subagent_type);
        assert_eq!(spawn_notif.has_child_view, spawn_update.has_child_view);
        assert_eq!(spawn_notif.scrollback_len, spawn_update.scrollback_len);
        assert_eq!(spawn_notif.child_session_id, child_sid);
        assert_eq!(spawn_update.child_session_id, child_sid);
        assert!(matches!(spawn_notif.block_kind, SubagentBlockKind::Started));
        assert!(matches!(
            spawn_update.block_kind,
            SubagentBlockKind::Started
        ));
        assert_eq!(
            spawn_notif.scrollback_entry_id,
            spawn_update.scrollback_entry_id
        );
        assert!(spawn_notif.scrollback_entry_id.is_some());

        assert!(finish_notif.finished);
        assert!(finish_update.finished);
        assert_eq!(finish_notif.status.as_deref(), Some("completed"));
        assert_eq!(finish_update.status.as_deref(), Some("completed"));
        assert_eq!(finish_notif.tool_calls, Some(2));
        assert_eq!(finish_update.tool_calls, Some(2));
        assert_eq!(finish_notif.turns, Some(1));
        assert_eq!(finish_update.turns, Some(1));
        assert_eq!(finish_notif.duration_ms, Some(500));
        assert_eq!(finish_update.duration_ms, Some(500));
        assert!(matches!(
            finish_notif.block_kind,
            SubagentBlockKind::Completed { .. }
        ));
        assert!(matches!(
            finish_update.block_kind,
            SubagentBlockKind::Completed { .. }
        ));
    }

    #[test]
    fn ext_session_update_for_inactive_agent_registers_subagent_without_redraw() {
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        switch_active_to(&mut app, AgentId(1));

        let child_sid = "child-inactive";
        let affected = handle(
            make_ext_session_notification_with_method(
                "sess-A",
                "grow/session/update",
                test_subagent_spawned("sess-A", child_sid),
            ),
            &mut app,
        );

        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        let info = agent_a
            .session.subagent_sessions
            .get(child_sid)
            .expect("SubagentSpawned must register on inactive agent A");
        assert!(
            agent_a.subagent_views.contains_key(child_sid),
            "SubagentSpawned must create subagent_views on inactive agent A"
        );
        assert_eq!(agent_a.scrollback.len(), 1);
        let entry_id = info
            .scrollback_entry_id
            .expect("inactive spawn must stash scrollback_entry_id");
        let entry = agent_a.scrollback.get_by_id(entry_id).unwrap();
        let RenderBlock::Subagent(sb) = &entry.block else {
            panic!("inactive spawn must push SubagentBlock");
        };
        assert!(matches!(sb.kind, SubagentBlockKind::Started));
        assert!(
            !affected,
            "SubagentSpawned on inactive agent must not request a redraw"
        );

        let affected = handle(
            make_ext_session_notification_with_method(
                "sess-A",
                "grow/session/update",
                test_subagent_finished(child_sid),
            ),
            &mut app,
        );
        assert!(
            !affected,
            "SubagentFinished on inactive agent must not request a redraw"
        );

        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        let info = agent_a.session.subagent_sessions.get(child_sid).unwrap();
        assert!(info.finished);
        assert_eq!(info.status.as_deref(), Some("completed"));
        let started = agent_a.scrollback.get_by_id(entry_id).unwrap();
        let RenderBlock::Subagent(started_block) = &started.block else {
            panic!("inactive finish must keep the started Subagent block");
        };
        assert!(matches!(started_block.kind, SubagentBlockKind::Started));
        let terminal = agent_a.scrollback.entry(1).unwrap();
        let RenderBlock::Subagent(terminal_block) = &terminal.block else {
            panic!("inactive finish must append a terminal Subagent block");
        };
        assert!(matches!(
            terminal_block.kind,
            SubagentBlockKind::Completed { .. }
        ));
    }

    #[test]
    fn ext_session_update_unknown_session_subagent_spawned_no_op() {
        let mut app = make_app_with_agent("sess-A");
        let affected = handle(
            make_ext_session_notification_with_method(
                "sess-unknown",
                "grow/session/update",
                test_subagent_spawned("sess-unknown", "child-unknown"),
            ),
            &mut app,
        );

        assert!(!affected, "unknown session_id must not request a redraw");
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert!(
            agent.session.subagent_sessions.is_empty(),
            "SubagentSpawned for unknown session must not register subagent_sessions"
        );
        assert!(
            agent.scrollback.is_empty(),
            "SubagentSpawned for unknown session must not push scrollback"
        );
    }

    #[test]
    fn ext_session_update_malformed_params_returns_false() {
        let mut app = make_app_with_agent("sess-A");
        let (tx, _rx) = tokio::sync::oneshot::channel();
        // Valid JSON but not a SessionNotification — parse must fail quietly.
        let raw =
            serde_json::value::to_raw_value(&serde_json::json!({"unexpected": true})).unwrap();
        let request = acp::ExtNotification::new("grow/session/update", raw.into());
        let msg = AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request,
            response_tx: tx,
        });

        let affected = handle(msg, &mut app);

        assert!(
            !affected,
            "malformed grow/session/update params must not redraw"
        );
        assert!(
            app.agents.get(&AgentId(0)).unwrap().scrollback.is_empty(),
            "malformed notification must not mutate scrollback"
        );
    }

    #[test]
    fn ext_session_notification_for_inactive_agent_updates_its_context_used() {
        // AutoCompactCompleted on the Grow ext path resets the context bar
        // numerator via refresh_context_used. That side effect must run on
        // the matched agent regardless of which view is currently active.
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));
        // Seed A with a stale context-used reading so we can prove the
        // notification reset it.
        {
            let agent_a = app.agents.get_mut(&AgentId(0)).unwrap();
            agent_a.apply_context_used(90_000, 131_072);
        }
        switch_active_to(&mut app, AgentId(1));

        let affected = handle(
            make_ext_session_notification(
                "sess-A",
                GrowSessionUpdate::AutoCompactCompleted {
                    manual: false,
                    async_compact: false,
                    tokens_before: 90_000,
                    tokens_after: 25_000,
                    elapsed_ms: Some(300),
                    summary_preview: None,
                },
            ),
            &mut app,
        );

        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent_a.session.context_state.as_ref().map(|c| c.used),
            Some(25_000),
            "AutoCompactCompleted must reset A's context_used even when B is active"
        );
        assert!(
            !affected,
            "ext notification routed to a non-active agent must not request a redraw"
        );
    }

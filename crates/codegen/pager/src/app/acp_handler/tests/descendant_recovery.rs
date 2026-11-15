use super::*;

fn write_empty_timeline(home: &std::path::Path, session_id: &str) -> std::path::PathBuf {
    let session_dir = write_session_summary(home, "/tmp", session_id, None);
    shell::session::testkit::write_timeline_history_fixture(&session_dir, &[], &[]).unwrap();
    session_dir
}

fn replay_spawn(app: &mut AppView, parent_sid: &str, child_sid: &str, event_id: &str) {
    let message = make_replayed_ext_session_notification(
        parent_sid,
        event_id,
        test_subagent_spawned(parent_sid, child_sid),
    );
    handle(message, app);
}

fn start_replay(app: &mut AppView) {
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
}

fn apply_descendant_restore_effect(app: &mut AppView, effects: Vec<crate::app::actions::Effect>) {
    let effect = effects
        .into_iter()
        .find(|effect| matches!(effect, crate::app::actions::Effect::RestoreDescendantState { .. }))
        .expect("session result schedules descendant restoration");
    let crate::app::actions::Effect::RestoreDescendantState {
        agent_id,
        session_id,
        binding_epoch,
        request_id,
        root_child_session_ids,
        child_control_captures,
        ownership,
        grow_home,
    } = effect
    else {
        unreachable!();
    };
    let batch = crate::app::subagent::read_descendant_replay_batch(
        session_id.0.as_ref(),
        &root_child_session_ids,
        ownership,
        &grow_home,
    );
    crate::app::subagent::apply_descendant_state_restore(
        app,
        agent_id,
        &session_id,
        binding_epoch,
        request_id,
        &child_control_captures,
        batch,
    );
}

fn write_child_lifecycle(
    home: &std::path::Path,
    parent_sid: &str,
    child_sid: &str,
    grandchild_sid: &str,
    lifecycle: shell::extensions::notification::SessionUpdate,
) {
    use shell::extensions::notification::SessionNotification;

    let parent_dir = home
        .join("sessions")
        .join(shell::util::grow_home::encode_cwd_dirname("/tmp"))
        .join(parent_sid);
    let parent_timeline =
        shell::session::testkit::read_timeline_fixture(&parent_dir.join("timeline.jsonl")).unwrap();
    let (spawn_seq, spawn) =
        parent_timeline
            .events()
            .iter()
            .find_map(|event| match &event.kind {
                chat_state::TimelineEventKind::Subagent(chat_state::SubagentEvent::Spawned(
                    spawn,
                )) if spawn.child_session_id == child_sid => Some((event.seq.get(), spawn)),
                _ => None,
            })
            .expect("parent has canonical child spawn");

    let child_dir = write_session_summary(home, "/tmp", child_sid, Some("subagent"));
    let summary_path = child_dir.join("summary.json");
    let mut summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&summary_path).unwrap()).unwrap();
    summary["parent_session_id"] = serde_json::Value::String(parent_sid.to_owned());
    std::fs::write(&summary_path, serde_json::to_vec(&summary).unwrap()).unwrap();

    let mut child_timeline = chat_state::Timeline::default();
    child_timeline
        .record(chat_state::TimelineEventKind::SubagentSeed(
            chat_state::SubagentSeedEvent {
                parent_timeline_id: parent_sid.to_owned(),
                parent_spawn_seq: spawn_seq,
                subagent_id: spawn.subagent_id.clone(),
                security_parent_session_id: parent_sid.to_owned(),
                context_source: spawn.context_source.clone(),
                source_ref: spawn.source_ref.clone(),
                normalized: spawn.context_normalized,
            },
        ))
        .unwrap();
    child_timeline
        .record(chat_state::TimelineEventKind::Subagent(
            chat_state::SubagentEvent::Spawned(Box::new(chat_state::SubagentSpawnEvent {
                subagent_id: grandchild_sid.to_owned(),
                child_session_id: grandchild_sid.to_owned(),
                origin_tool_call_id: None,
                security_parent_session_id: child_sid.to_owned(),
                parent_step_id: None,
                subagent_type: "general-purpose".into(),
                description: "nested task".into(),
                prompt: "nested prompt".into(),
                execution_recipe: serde_json::json!({}),
                context_source: chat_state::SubagentContextSource::New,
                source_ref: None,
                context_normalized: false,
                resumed_from: None,
                parent_prompt_id: None,
                capability_mode: None,
                birth_mcp_servers: Some(Default::default()),
                permission_mode: None,
                effective_permission_mode: None,
                workflow_run_id: None,
                goal_id: None,
                goal_definition_revision: None,
                surface_completion: true,
                child_cwd: "/tmp".into(),
                worktree_path: None,
                effective_model_id: "grow-3".into(),
                model_transport_key: tools::types::resources::ModelImageInputKey::new(
                    "grow-3",
                    "responses",
                    "test-endpoint",
                ),
                reasoning_effort: None,
            })),
        ))
        .unwrap();
    shell::session::testkit::write_timeline_history_fixture(
        &child_dir,
        child_timeline.events(),
        &[shell::session::storage::SessionUpdate::Grow(Box::new(
            SessionNotification {
                session_id: acp::SessionId::new(child_sid),
                update: lifecycle,
                meta: None,
            },
        ))],
    )
    .unwrap();
}

#[test]
fn captured_ownership_enriches_existing_replayed_child_after_disk_disappears() {
    with_replay_disk_home(|home| {
        let root_sid = "captured-root";
        let child_sid = "captured-child";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "captured canonical prompt");
        let captured = shell::session::storage::load_descendant_ownership_at(root_sid, home)
            .unwrap()
            .unwrap();

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "captured-root-1");
        let before = app.agents[&AgentId(0)].scrollback.len();

        let timeline = home
            .join("sessions")
            .join(shell::util::grow_home::encode_cwd_dirname("/tmp"))
            .join(root_sid)
            .join("timeline.jsonl");
        std::fs::remove_file(timeline).unwrap();
        // Exercise the normal load-result handoff, not only the helper API.
        let effects = crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::TaskComplete(
                crate::app::actions::TaskResult::SessionLoaded {
                    agent_id: AgentId(0),
                    session_id: acp::SessionId::new(root_sid),
                    models: None,
                    code_restored: false,
                    restore_summary: None,
                    foreground: None,
                    descendant_ownership: Some(captured),
                    older_history_cursor: None,
                    latest_history_cursor: None,
                },
            ),
            &mut app,
        );
        apply_descendant_restore_effect(&mut app, effects);

        let root = &app.agents[&AgentId(0)];
        assert_eq!(
            root.session.subagent_sessions[child_sid].prompt.as_deref(),
            Some("captured canonical prompt")
        );
        assert_eq!(
            root.session.subagent_sessions[child_sid]
                .child_cwd
                .as_deref(),
            Some("/tmp")
        );
        assert_eq!(
            root.scrollback.len(),
            before,
            "enrichment adds no duplicate UI row"
        );
    });
}

#[test]
fn terminal_only_page_bootstraps_exact_child_view_without_a_started_row() {
    with_replay_disk_home(|home| {
        let root_sid = "terminal-page-root";
        let child_sid = "terminal-page-child";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "canonical terminal child");
        let ownership = shell::session::storage::load_descendant_ownership_at(root_sid, home)
            .unwrap()
            .unwrap();
        let mut root_spawns = ownership.into_spawns();
        let spawn = root_spawns.remove(child_sid).unwrap();

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        handle(
            make_replayed_ext_session_notification(
                root_sid,
                "terminal-page-finished",
                test_subagent_finished(child_sid),
            ),
            &mut app,
        );
        let root = &app.agents[&AgentId(0)];
        assert!(!root.session.subagent_sessions.contains_key(child_sid));
        assert!(!root.subagent_views.contains_key(child_sid));
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = false;

        crate::app::subagent::apply_descendant_replay_batch(
            &mut app,
            AgentId(0),
            &[],
            crate::app::subagent::DescendantReplayBatch {
                root_spawns: std::collections::BTreeMap::from([(child_sid.to_owned(), spawn)]),
                replayed_direct_children: std::collections::HashSet::from([child_sid.to_owned()]),
                nodes: Vec::new(),
            },
        );

        let root = &app.agents[&AgentId(0)];
        assert!(root.subagent_views.contains_key(child_sid));
        assert!(root.session.subagent_sessions[child_sid].finished);
        assert!(!root.session.subagent_sessions[child_sid].child_updates_replayed);
        let rows = root
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| {
                matches!(
                    &entry.block,
                    crate::scrollback::block::RenderBlock::Subagent(block)
                        if block.child_session_id == child_sid
                )
            })
            .count();
        assert_eq!(rows, 1, "the terminal row remains the only visible lifecycle block");
    });
}

#[test]
fn foreign_ownership_is_rejected_and_local_validated_facts_win() {
    with_replay_disk_home(|home| {
        let root_sid = "identity-root";
        let foreign_sid = "identity-foreign";
        let child_sid = "identity-child";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "root canonical prompt");
        write_subagent_spawn_timeline(home, foreign_sid, child_sid, "foreign forged prompt");
        let foreign = shell::session::storage::load_descendant_ownership_at(foreign_sid, home)
            .unwrap()
            .unwrap();

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "identity-root-1");
        crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), Some(foreign));

        assert_eq!(
            app.agents[&AgentId(0)].session.subagent_sessions[child_sid]
                .prompt
                .as_deref(),
            Some("root canonical prompt")
        );
    });
}

#[test]
fn verified_empty_ownership_is_a_snapshot_and_none_reloads_new_root_facts() {
    with_replay_disk_home(|home| {
        let root_sid = "empty-captured-root";
        let child_sid = "appended-child";
        write_empty_timeline(home, root_sid);
        let captured_empty = shell::session::storage::load_descendant_ownership_at(root_sid, home)
            .unwrap()
            .unwrap();
        assert!(captured_empty.clone().into_spawns().is_empty());

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        // Disk has gained a canonical spawn since the response snapshot, and
        // replay independently supplies its UI row.
        write_subagent_spawn_timeline(home, root_sid, child_sid, "later canonical prompt");
        replay_spawn(&mut app, root_sid, child_sid, "empty-captured-root-1");

        crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), Some(captured_empty));
        assert_eq!(
            app.agents[&AgentId(0)].session.subagent_sessions[child_sid]
                .prompt
                .as_deref(),
            None,
            "a verified empty snapshot cannot absorb later disk writes"
        );

        crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);
        assert_eq!(
            app.agents[&AgentId(0)].session.subagent_sessions[child_sid]
                .prompt
                .as_deref(),
            Some("later canonical prompt")
        );
    });
}

#[test]
fn ui_only_child_cannot_authorize_nested_disk_descendants() {
    with_replay_disk_home(|home| {
        let root_sid = "ui-only-root";
        let child_sid = "ui-only-child";
        let grandchild_sid = "ui-only-grandchild";
        write_empty_timeline(home, root_sid);
        // A nested canonical spawn exists, but without a canonical root spawn
        // the root's replay snapshot grants no path to inspect this child log.
        write_subagent_spawn_timeline(
            home,
            child_sid,
            grandchild_sid,
            "unauthorized nested prompt",
        );

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "ui-only-root-1");
        replay_spawn(&mut app, child_sid, grandchild_sid, "ui-only-child-1");
        crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);

        let root = &app.agents[&AgentId(0)];
        assert_eq!(
            root.session.subagent_sessions[child_sid].prompt.as_deref(),
            None
        );
        assert_eq!(
            root.session.subagent_sessions[grandchild_sid]
                .prompt
                .as_deref(),
            None,
            "a UI-only parent row cannot grant nested ownership"
        );
        assert!(root.subagent_views.contains_key(grandchild_sid));
    });
}

#[test]
fn resync_ownership_is_consumed_only_after_matching_generation_finishes() {
    with_replay_disk_home(|home| {
        let root_sid = "generation-root";
        let child_sid = "generation-child";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "current boundary prompt");
        let captured = shell::session::storage::load_descendant_ownership_at(root_sid, home)
            .unwrap()
            .unwrap();
        let mut app = make_app_with_agent(root_sid);
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .begin_session_reload(9);
        replay_spawn(&mut app, root_sid, child_sid, "generation-root-1");
        for generation in [8, 9] {
            let effects = crate::app::root::dispatch::dispatch(
                crate::app::actions::Action::TaskComplete(
                    crate::app::actions::TaskResult::SessionResynced {
                        agent_id: AgentId(0),
                        session_id: acp::SessionId::new(root_sid),
                        generation,
                        result: Ok(None),
                        descendant_ownership: Some(captured.clone()),
                    },
                ),
                &mut app,
            );
            if generation == 8 {
                let root = &app.agents[&AgentId(0)];
                assert!(root.session_reload.is_some());
                assert_eq!(
                    root.session.subagent_sessions[child_sid].prompt.as_deref(),
                    None
                );
            } else {
                assert!(app.agents[&AgentId(0)].session_reload.is_none());
                apply_descendant_restore_effect(&mut app, effects);
                let root = &app.agents[&AgentId(0)];
                assert_eq!(
                    root.session.subagent_sessions[child_sid].prompt.as_deref(),
                    Some("current boundary prompt")
                );
            }
        }
    });
}

#[test]
fn descendant_replay_is_offloaded_and_preserves_live_controls_and_cursor() {
    with_replay_disk_home(|home| {
        use shell::extensions::notification::{SessionNotification, SessionUpdate};

        let root_sid = "async-descendant-root";
        let child_sid = "async-descendant-child";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "canonical prompt");
        let ownership = shell::session::storage::load_descendant_ownership_at(root_sid, home)
            .unwrap()
            .unwrap();
        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "async-descendant-root-1");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = false;

        let first = crate::app::subagent::schedule_descendant_state_restore(
            &mut app,
            AgentId(0),
            Some(ownership.clone()),
        );
        assert_eq!(first.len(), 1);
        let first_request = match &first[0] {
            crate::app::actions::Effect::RestoreDescendantState { request_id, .. } => *request_id,
            other => panic!("unexpected effect: {other:?}"),
        };
        assert!(!app.agents[&AgentId(0)].session.loading_replay);
        assert!(app.agents[&AgentId(0)].descendant_restore_pending);

        let live_update = SessionNotification {
            session_id: acp::SessionId::new(child_sid),
            update: SessionUpdate::AgentChanged {
                agent_name: "reviewer".into(),
            },
            meta: None,
        };
        let raw = serde_json::value::to_raw_value(&live_update).unwrap();
        let live_ext = acp::ExtNotification::new("grow/session_notification", raw.into());
        assert!(handle_session_notification(&live_ext, &mut app));

        let second = crate::app::subagent::schedule_descendant_state_restore(
            &mut app,
            AgentId(0),
            Some(ownership),
        );
        let (second_request, second_captures) = match &second[0] {
            crate::app::actions::Effect::RestoreDescendantState {
                request_id,
                child_control_captures,
                ..
            } => (*request_id, child_control_captures.clone()),
            other => panic!("unexpected effect: {other:?}"),
        };
        let binding_epoch = app.agents[&AgentId(0)].session_binding_epoch;
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .replay_live_cursor_seen = true;
        let stale_control = SessionNotification {
            session_id: acp::SessionId::new(child_sid),
            update: SessionUpdate::AgentChanged {
                agent_name: "coder".into(),
            },
            meta: Some(serde_json::json!({ "isReplay": true, "eventId": "old-agent" })),
        };
        let stale_batch = crate::app::subagent::DescendantReplayBatch {
            nodes: vec![crate::app::subagent::DescendantReplayNode {
                parent_session_id: child_sid.into(),
                durable_children: Default::default(),
                lifecycle: vec![stale_control],
            }],
            ..Default::default()
        };
        crate::app::subagent::apply_descendant_state_restore(
            &mut app,
            AgentId(0),
            &acp::SessionId::new(root_sid),
            binding_epoch,
            first_request,
            &[],
            crate::app::subagent::DescendantReplayBatch::default(),
        );
        assert!(app.agents[&AgentId(0)].descendant_restore_pending);
        assert_eq!(app.agents[&AgentId(0)].subagent_views[child_sid].session.agent_name(), Some("reviewer"));

        let binding_epoch = app.agents[&AgentId(0)].session_binding_epoch;
        crate::app::subagent::apply_descendant_state_restore(
            &mut app,
            AgentId(0),
            &acp::SessionId::new(root_sid),
            binding_epoch,
            second_request,
            &second_captures,
            stale_batch,
        );
        let root = &app.agents[&AgentId(0)];
        assert!(!root.descendant_restore_pending);
        assert_eq!(root.subagent_views[child_sid].session.agent_name(), Some("reviewer"));
        assert!(root.session.replay_live_cursor_seen);
    });
}

#[test]
fn forced_restore_reuses_idless_started_row_for_canonical_descendant() {
    with_replay_disk_home(|home| {
        let root_sid = "idless-root";
        let child_sid = "idless-child";
        let grandchild_sid = "idless-grandchild";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "child prompt");
        write_child_lifecycle(
            home,
            root_sid,
            child_sid,
            grandchild_sid,
            test_subagent_spawned(child_sid, grandchild_sid),
        );

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "idless-root-1");
        for _ in 0..2 {
            crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);
        }

        let rows = app.agents[&AgentId(0)]
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| {
                matches!(
                    &entry.block,
                    RenderBlock::Subagent(block)
                        if block.child_session_id == grandchild_sid
                            && matches!(
                                &block.kind,
                                crate::scrollback::blocks::SubagentBlockKind::Started
                            )
                )
            })
            .count();
        assert_eq!(rows, 1);
    });
}

#[test]
fn descendant_lifecycle_with_mismatched_identity_is_not_projected() {
    with_replay_disk_home(|home| {
        let root_sid = "mismatch-root";
        let child_sid = "mismatch-child";
        let grandchild_sid = "mismatch-grandchild";
        write_subagent_spawn_timeline(home, root_sid, child_sid, "child prompt");
        write_child_lifecycle(
            home,
            root_sid,
            child_sid,
            grandchild_sid,
            test_subagent_spawned("spoofed-parent", grandchild_sid),
        );

        let mut app = make_app_with_agent(root_sid);
        start_replay(&mut app);
        replay_spawn(&mut app, root_sid, child_sid, "mismatch-root-1");
        crate::app::subagent::restore_descendant_state(&mut app, AgentId(0), None);

        let root = &app.agents[&AgentId(0)];
        assert!(!root.session.subagent_sessions.contains_key(grandchild_sid));
        assert!(!root.subagent_views.contains_key(grandchild_sid));
        assert!(!root.scrollback.iter_entries().any(|(_, entry)| {
            matches!(
                &entry.block,
                RenderBlock::Subagent(block) if block.child_session_id == grandchild_sid
            )
        }));
    });
}

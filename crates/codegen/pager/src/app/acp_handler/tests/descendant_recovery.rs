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
        crate::app::root::dispatch::dispatch(
            crate::app::actions::Action::TaskComplete(
                crate::app::actions::TaskResult::SessionLoaded {
                    agent_id: AgentId(0),
                    session_id: acp::SessionId::new(root_sid),
                    models: None,
                    code_restored: false,
                    restore_summary: None,
                    foreground: None,
                    descendant_ownership: Some(captured),
                },
            ),
            &mut app,
        );

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
            crate::app::root::dispatch::dispatch(
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
            let root = &app.agents[&AgentId(0)];
            if generation == 8 {
                assert!(root.session_reload.is_some());
                assert_eq!(
                    root.session.subagent_sessions[child_sid].prompt.as_deref(),
                    None
                );
            } else {
                assert!(root.session_reload.is_none());
                assert_eq!(
                    root.session.subagent_sessions[child_sid].prompt.as_deref(),
                    Some("current boundary prompt")
                );
            }
        }
    });
}

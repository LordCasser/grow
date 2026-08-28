#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    /// Regression: a machine-wide `grow/models/update` broadcast
    /// carries each model's static catalog-default effort (`high`), not the
    /// session's chosen `xhigh`, and must not clobber the per-session choice.
    #[test]
    fn models_update_preserves_user_reasoning_effort() {
        use shell::sampling::types::ReasoningEffort;
        let mut app = make_app_with_agent("sess-1");

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let id = acp::ModelId::new(std::sync::Arc::from("reason-model"));
        let mut info = make_model_info("reason-model");
        info.meta = serde_json::json!({
            "reasoningEffort": "high",
            "reasoningEfforts": ["high", "xhigh"],
        })
        .as_object()
        .cloned();
        agent.session.models.available.insert(id.clone(), info);
        agent
            .session
            .models
            .set_current(id, Some(ReasoningEffort::Xhigh));
        assert_eq!(
            agent.session.models.reasoning_effort,
            Some(ReasoningEffort::Xhigh)
        );

        let notif = make_reasoning_models_update_notif("reason-model", "high");
        assert!(handle_models_update(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.session.models.reasoning_effort,
            Some(ReasoningEffort::Xhigh),
            "models/update broadcast must not clobber a user-set per-session effort"
        );
    }

    #[test]
    fn models_update_refreshes_default_and_preserves_active_agent_model() {
        let mut app = make_app_with_agent("sess-1");

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let id_3 = acp::ModelId::new(std::sync::Arc::from("grow-3"));
        agent
            .session
            .models
            .available
            .insert(id_3.clone(), make_model_info("grow-3"));
        agent.session.models.current = Some(id_3.clone());
        app.models.available.insert(id_3.clone(), make_model_info("grow-3"));
        app.models.current = Some(id_3);

        let notif = make_models_update_notif("grow-4", &["grow-3", "grow-4"]);
        handle_models_update(&notif, &mut app);

        assert_eq!(
            app.models.current.as_ref().map(|id| id.0.as_ref()),
            Some("grow-4"),
            "app.models is the new-session template and must follow the shell default"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-3"),
            "agent's per-session model must be preserved"
        );
    }

    #[test]
    fn models_update_uses_shell_default_when_agent_model_removed() {
        let mut app = make_app_with_agent("sess-1");

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let id_3 = acp::ModelId::new(std::sync::Arc::from("grow-3"));
        agent
            .session
            .models
            .available
            .insert(id_3.clone(), make_model_info("grow-3"));
        agent.session.models.current = Some(id_3);

        // grow-3 removed from catalog.
        let notif = make_models_update_notif("grow-4.3", &["grow-4.3", "grow-4.5"]);
        handle_models_update(&notif, &mut app);

        assert_eq!(
            app.models.current.as_ref().map(|id| id.0.as_ref()),
            Some("grow-4.3"),
            "app.models.current must use shell default when agent model removed"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-4.3"),
            "agent must fall back to shell default when its model is removed"
        );
    }

    #[test]
    fn models_update_without_active_agent_uses_shell_default() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = AppView::new(tx, ModelState::default(), Vec::new());

        let notif = make_models_update_notif("grow-4", &["grow-3", "grow-4"]);
        handle_models_update(&notif, &mut app);

        assert_eq!(
            app.models.current.as_ref().map(|id| id.0.as_ref()),
            Some("grow-4"),
            "without an active agent, shell default must be used"
        );
    }

    #[test]
    fn models_update_noop_when_agent_matches_shell_default() {
        let mut app = make_app_with_agent("sess-1");

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let id_4 = acp::ModelId::new(std::sync::Arc::from("grow-4"));
        agent
            .session
            .models
            .available
            .insert(id_4.clone(), make_model_info("grow-4"));
        agent.session.models.current = Some(id_4);

        let notif = make_models_update_notif("grow-4", &["grow-3", "grow-4"]);
        handle_models_update(&notif, &mut app);

        assert_eq!(
            app.models.current.as_ref().map(|id| id.0.as_ref()),
            Some("grow-4"),
            "app.models.current must be grow-4 when agent and shell agree"
        );
        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-4"),
            "agent model must remain grow-4"
        );
    }

    #[test]
    fn models_update_non_active_agent_uses_shell_fallback_not_active_model() {
        let mut app = make_app_with_agent("sess-A");
        insert_agent(&mut app, AgentId(1), Some("sess-B"));

        {
            let agent_a = app.agents.get_mut(&AgentId(0)).unwrap();
            let id_3 = acp::ModelId::new(std::sync::Arc::from("grow-3"));
            agent_a
                .session
                .models
                .available
                .insert(id_3.clone(), make_model_info("grow-3"));
            agent_a.session.models.current = Some(id_3);
        }

        {
            let agent_b = app.agents.get_mut(&AgentId(1)).unwrap();
            let id_5 = acp::ModelId::new(std::sync::Arc::from("grow-4.5"));
            agent_b
                .session
                .models
                .available
                .insert(id_5.clone(), make_model_info("grow-4.5"));
            agent_b.session.models.current = Some(id_5);
        }

        // grow-5 removed from catalog.
        let notif = make_models_update_notif("grow-4", &["grow-3", "grow-4"]);
        handle_models_update(&notif, &mut app);

        assert_eq!(
            app.models.current.as_ref().map(|id| id.0.as_ref()),
            Some("grow-4"),
        );
        let agent_a = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent_a
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-3"),
            "agent A's model must be preserved"
        );

        // B's grow-5 was removed — must fall back to shell's grow-4, not A's grow-3.
        let agent_b = app.agents.get(&AgentId(1)).unwrap();
        assert_eq!(
            agent_b
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-4"),
            "inactive agent must fall back to shell default, not active agent's model"
        );
    }

    /// A follower client (no in-flight switch of its own) receives the
    /// leader's `ModelChanged` broadcast and silently mirrors the new model
    /// into its local state — no scrollback entry, no toast, just enough
    /// state for the status bar / `/model` dropdown to render correctly.
    #[test]
    fn model_changed_updates_state_silently_on_follower() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);
        let scrollback_before = agent.scrollback.len();
        // Follower: no local switch in flight.
        assert!(!agent.session.model_switch_pending());

        let notif = model_changed_ext("sess-1", "grow-4", None);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            changed,
            "follower's state changed → handler must request a redraw"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-4"),
            "follower must mirror the remote switch into its local model state",
        );
        assert_eq!(
            agent.scrollback.len(),
            scrollback_before,
            "follower must NOT push a 'Switched to' scrollback entry — that is \
             the invoking client's job (SwitchModelComplete owns the system message)"
        );
        assert!(
            !agent.session.model_switch_pending(),
            "follower's pending flag must stay false (no local switch was issued)"
        );
    }

    /// A live remote `ModelChanged` (leader-mode fan-out from another client)
    /// must apply even when this client already has a local
    /// `user_model_preference` — otherwise the status bar desyncs from the
    /// gateway session. Preference is updated to track the new live model.
    /// (History-replay silent-revert is suppressed on the shell side via
    /// `ReconnectState::user_selected_model`, not by permanently blocking
    /// remote ModelChanged here.)
    #[test]
    fn model_changed_applies_and_updates_user_model_preference() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "heavy", &["auto", "heavy"]);
        agent.session.user_model_preference =
            Some(acp::ModelId::new(std::sync::Arc::from("heavy")));
        assert!(!agent.session.model_switch_pending());

        let notif = model_changed_ext("sess-1", "auto", None);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            changed,
            "remote live ModelChanged must apply despite prior local preference"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("auto"),
            "selector must mirror the remote switch"
        );
        assert_eq!(
            agent
                .session
                .user_model_preference
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("auto"),
            "preference must track the applied remote switch"
        );
    }

    /// The invoking client is also a subscriber to its own session and so
    /// receives the broadcast it triggered. Its in-flight
    /// `SetSessionModelResponse` is the authority for its local state +
    /// the single "Switched to X" scrollback entry, so the broadcast handler
    /// must be a no-op here — gated on `model_switch_pending == true`.
    ///
    /// Concretely we verify the broadcast does NOT touch
    /// `models.current` (preserving the pre-response snapshot) — that
    /// snapshot is what `SwitchModelComplete`'s `unchanged` check compares
    /// against to decide whether to render the "Switched to X" message. If
    /// the broadcast optimistically updated state here, the response
    /// handler would see `prev == new`, mark it unchanged, and suppress the
    /// user-facing message entirely.
    #[test]
    fn model_changed_skipped_when_local_switch_in_flight() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);
        // Invoker: a local switch is in flight (set by Action::SwitchModel /
        // set_default_model before the SetSessionModelRequest is sent).
        agent.session.begin_model_switch_for_test();
        let scrollback_before = agent.scrollback.len();

        let notif = model_changed_ext("sess-1", "grow-4", None);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            !changed,
            "broadcast must be a no-op while local switch is pending"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-3"),
            "models.current must stay at the pre-response snapshot — \
             SwitchModelComplete owns the final apply + system message"
        );
        assert_eq!(
            agent.scrollback.len(),
            scrollback_before,
            "broadcast must not push any scrollback entry on the invoker"
        );
        assert!(
            agent.session.model_switch_pending(),
            "pending flag must remain set until SwitchModelComplete arrives"
        );
    }

    #[test]
    fn model_changed_is_applied_after_the_local_control_queue_drains() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);
        let token = agent.session.begin_model_switch_for_test();

        let notif = model_changed_ext("sess-1", "grow-4", Some("high"));
        assert!(
            !handle_ext_notification(&notif, &mut app),
            "the notification is deferred until the local control terminal"
        );
        assert_eq!(
            app.agents[&AgentId(0)].session.models.current.as_ref().unwrap().0.as_ref(),
            "grow-3"
        );

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert_eq!(
            agent.session.complete_control(token),
            crate::app::session::SessionControlCompletion::Drained
        );
        assert!(apply_deferred_authoritative_controls(agent, "sess-1"));
        assert_eq!(
            agent.session.models.current.as_ref().unwrap().0.as_ref(),
            "grow-4",
            "the newest server state wins once the local queue is terminal"
        );
    }

    #[test]
    fn agent_changed_is_not_blocked_by_an_unrelated_sampling_control() {
        let mut app = make_app_with_agent("sess-1");
        let token = app
            .agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .begin_model_switch_for_test();

        let notif = agent_changed_ext("sess-1", "reviewer");
        assert!(handle_ext_notification(&notif, &mut app));
        assert_eq!(app.agents[&AgentId(0)].session.agent_name(), Some("reviewer"));

        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert_eq!(
            agent.session.complete_control(token),
            crate::app::session::SessionControlCompletion::Drained
        );
        assert!(
            !apply_deferred_authoritative_controls(agent, "sess-1"),
            "draining Sampling must not re-apply an Agent event that already committed"
        );
        assert_eq!(agent.session.agent_name(), Some("reviewer"));
    }

    /// A `ModelChanged` can race ahead of the matching catalog generation.
    /// It remains authoritative, but must not be projected until the model
    /// metadata exists locally.
    #[test]
    fn model_changed_waits_for_matching_catalog_generation() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);

        let notif = model_changed_ext("sess-1", "grow-99-unknown", None);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            !changed,
            "unknown model must NOT trigger a redraw — no state changed"
        );

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-3"),
            "models.current must stay on the previously-known model"
        );

        let update = make_models_update_notif(
            "grow-99-unknown",
            &["grow-3", "grow-4", "grow-99-unknown"],
        );
        assert!(handle_models_update(&update, &mut app));
        assert_eq!(
            app.agents[&AgentId(0)]
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-99-unknown"),
            "catalog publication must retry the held authoritative state"
        );
    }

    /// `reasoning_effort` round-trips through the broadcast: the follower
    /// applies it alongside the model id so the prompt header / status bar
    /// show the right effort without waiting for a subsequent
    /// `grow/models/update`.
    #[test]
    fn model_changed_applies_reasoning_effort_on_follower() {
        use shell::sampling::types::ReasoningEffort;
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);

        let notif = model_changed_ext("sess-1", "grow-4", Some("high"));
        assert!(handle_ext_notification(&notif, &mut app));

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent.session.models.reasoning_effort,
            Some(ReasoningEffort::High),
            "follower must mirror the broadcast's reasoning_effort"
        );
    }

    /// `ModelChanged` for a session this client doesn't own / hasn't loaded
    /// must be dropped — `find_session_match` returns `None`. The bug-flavored
    /// version of this would be: leader-mode A switches model on session X
    /// (which this client never opened) and we accidentally apply the change
    /// to the active agent.
    #[test]
    fn model_changed_dropped_for_unknown_session_id() {
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        seed_models(agent, "grow-3", &["grow-3", "grow-4"]);

        let notif = model_changed_ext("sess-OTHER", "grow-4", None);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(!changed);

        let agent = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(
            agent
                .session
                .models
                .current
                .as_ref()
                .map(|id| id.0.as_ref()),
            Some("grow-3"),
            "unrelated-session broadcast must not touch this agent's model"
        );
    }

    #[test]
    fn model_changed_broadcast_updates_only_the_target_child() {
        let mut app = make_app_with_agent("root-session");
        seed_models(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "grow-3",
            &["grow-3", "grow-4"],
        );
        let spawned = make_ext_session_notification(
            "root-session",
            test_subagent_spawned("root-session", "child-session"),
        );
        let AcpClientMessage::ExtNotification(spawned) = spawned else {
            panic!("helper must produce an extension notification");
        };
        assert!(handle_ext_notification(&spawned.request, &mut app));

        let changed = model_changed_ext("child-session", "grow-4", Some("high"));
        assert!(handle_ext_notification(&changed, &mut app));

        let root = app.agents.get(&AgentId(0)).unwrap();
        assert_eq!(root.session.models.current.as_ref().unwrap().0.as_ref(), "grow-3");
        assert_eq!(
            root.subagent_views["child-session"]
                .session
                .models
                .current
                .as_ref()
                .unwrap()
                .0
                .as_ref(),
            "grow-4"
        );
    }

    #[test]
    fn models_update_refreshes_child_catalog_without_clobbering_its_selection() {
        let mut app = make_app_with_agent("root-session");
        seed_models(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "grow-3",
            &["grow-3", "grow-4"],
        );
        let spawned = make_ext_session_notification(
            "root-session",
            test_subagent_spawned("root-session", "child-session"),
        );
        let AcpClientMessage::ExtNotification(spawned) = spawned else {
            panic!("helper must produce an extension notification");
        };
        assert!(handle_ext_notification(&spawned.request, &mut app));
        let child = app.agents.get_mut(&AgentId(0)).unwrap().subagent_views
            .get_mut("child-session")
            .unwrap();
        seed_models(child, "grow-3", &["grow-3", "grow-4"]);

        let update = make_models_update_notif("grow-4", &["grow-3", "grow-4", "grow-5"]);
        assert!(handle_models_update(&update, &mut app));

        let child = &app.agents[&AgentId(0)].subagent_views["child-session"];
        assert_eq!(child.session.models.current.as_ref().unwrap().0.as_ref(), "grow-3");
        assert!(child
            .session
            .models
            .available
            .contains_key(&acp::ModelId::new("grow-5")));
        assert!(handle_ext_notification(
            &model_changed_ext("child-session", "grow-5", None),
            &mut app
        ));
        assert_eq!(
            app.agents[&AgentId(0)].subagent_views["child-session"]
                .session
                .models
                .current
                .as_ref()
                .unwrap()
                .0
                .as_ref(),
            "grow-5",
            "the child can resolve the authoritative model after a catalog refresh"
        );
    }

    #[test]
    fn child_model_changed_uses_its_own_event_highwater() {
        let mut app = make_app_with_agent("root-session");
        seed_models(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "grow-3",
            &["grow-3", "grow-4", "grow-5"],
        );
        let spawned = make_ext_session_notification(
            "root-session",
            test_subagent_spawned("root-session", "child-session"),
        );
        let AcpClientMessage::ExtNotification(spawned) = spawned else {
            panic!("helper must produce an extension notification");
        };
        assert!(handle_ext_notification(&spawned.request, &mut app));

        assert!(handle_ext_notification(
            &model_changed_ext_with_event("child-session", "grow-5", "child-session-12"),
            &mut app
        ));
        assert!(!handle_ext_notification(
            &model_changed_ext_with_event("child-session", "grow-4", "child-session-11"),
            &mut app
        ));

        let child = &app.agents[&AgentId(0)].subagent_views["child-session"];
        assert_eq!(child.session.models.current.as_ref().unwrap().0.as_ref(), "grow-5");
        assert_eq!(child.session.last_applied_grow_event_seq, Some(12));
        assert_eq!(
            child.session.last_seen_event_id.as_deref(),
            Some("child-session-12")
        );
    }

    #[test]
    fn workflow_pinned_child_ignores_process_catalog_reload() {
        let mut app = make_app_with_agent("root-session");
        seed_models(
            app.agents.get_mut(&AgentId(0)).unwrap(),
            "grow-3",
            &["grow-3", "grow-4"],
        );
        let mut update = test_subagent_spawned("root-session", "workflow-child");
        let GrowSessionUpdate::SubagentSpawned {
            workflow_run_id,
            model_state,
            workflow_agent_names,
            ..
        } = &mut update
        else {
            unreachable!()
        };
        *workflow_run_id = Some("run-1".to_string());
        *model_state = Some(acp::SessionModelState::new(
            acp::ModelId::new("grow-3"),
            vec![make_model_info("grow-3"), make_model_info("grow-4")],
        ));
        *workflow_agent_names = Some(vec!["reviewer".into(), "researcher".into()]);
        let spawned = make_ext_session_notification("root-session", update);
        let AcpClientMessage::ExtNotification(spawned) = spawned else {
            panic!("helper must produce an extension notification");
        };
        assert!(handle_ext_notification(&spawned.request, &mut app));
        let catalog = make_models_update_notif("grow-5", &["grow-4", "grow-5"]);
        assert!(handle_models_update(&catalog, &mut app));

        let child = &app.agents[&AgentId(0)].subagent_views["workflow-child"];
        assert_eq!(child.session.models.current.as_ref().unwrap().0.as_ref(), "grow-3");
        assert!(child
            .session
            .models
            .available
            .contains_key(&acp::ModelId::new("grow-3")));
        assert!(!child
            .session
            .models
            .available
            .contains_key(&acp::ModelId::new("grow-5")));
        assert_eq!(
            child.session.workflow_agent_names.as_deref(),
            Some(["reviewer".to_string(), "researcher".to_string()].as_slice())
        );
    }

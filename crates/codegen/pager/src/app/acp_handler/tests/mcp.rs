#![cfg_attr(rustfmt, rustfmt::skip)]
    use super::*;

    #[test]
    fn mcp_init_progress_updates_seeded_progress_in_place() {
        // When a session is seeded with mcp_init_progress{0,0}, a
        // subsequent init_progress notification must update total and
        // connected IN PLACE — preserving started_at for timer accuracy.
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.update_mcp_init_progress(0, 0);
        let started_at = agent.session.mcp_init_progress().unwrap().started_at;

        let notif = make_mcp_init_progress_notif(5, 0);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(changed);

        let progress = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!(progress.total, 5, "total must be updated from shell");
        assert_eq!(progress.connected, 0);
        assert_eq!(
            progress.started_at, started_at,
            "started_at must be preserved (timer anchoring)",
        );
    }

    #[test]
    fn server_status_routes_to_owning_agent() {
        use crate::views::extensions_modal::TabDataState;
        use crate::views::mcps_modal::McpServerDisplayStatus;
        use shell::extensions::mcp::McpServerStatus;

        // Agent 0 owns sess-owner; Agent 1 is foregrounded.
        let mut app = make_app_two_agents();
        seed_owner_agent_with_open_modal(&mut app);
        // Give the active agent its own modal so we can prove it's untouched.
        {
            let active = app.agents.get_mut(&AgentId(1)).unwrap();
            active.extensions_modal = Some(make_mcps_modal_with_servers(vec![
                crate::views::mcps_modal::McpServerInfo {
                    name: "alpha".into(),
                    display_name: None,
                    status: McpServerDisplayStatus::Initializing,
                    tool_count: 0,
                    setup_required: false,
                    setup: None,
                    setup_values: std::collections::HashMap::new(),
                    tools: Vec::new(),
                    enabled: true,
                    source: "local".into(),
                    plugin_name: None,
                },
            ]));
        }

        let tools = Some(vec![
            shell::extensions::mcp::McpToolEntry {
                name: "t1".into(),
                display_name: None,
                description: Some("one".into()),
                meta: None,
                enabled: true,
            },
            shell::extensions::mcp::McpToolEntry {
                name: "t2".into(),
                display_name: None,
                description: None,
                meta: None,
                enabled: true,
            },
        ]);
        let notif = make_server_status_notif("sess-owner", "alpha", McpServerStatus::Ready, tools);
        let redraw = handle_mcp_server_status(&notif, &mut app);
        assert!(
            !redraw,
            "owner is background — mutation must not request a redraw"
        );

        // Owner mutated.
        let owner_modal = app
            .agents
            .get(&AgentId(0))
            .unwrap()
            .extensions_modal
            .as_ref()
            .unwrap();
        let TabDataState::Loaded(ref owner_servers) = owner_modal.mcps_data else {
            panic!("owner modal must still be in Loaded state");
        };
        assert_eq!(owner_servers[0].status, McpServerDisplayStatus::Ready);
        assert_eq!(owner_servers[0].tool_count, 2);
        assert_eq!(owner_servers[0].tools.len(), 2);

        // Active agent's modal must be untouched.
        let active_modal = app
            .agents
            .get(&AgentId(1))
            .unwrap()
            .extensions_modal
            .as_ref()
            .unwrap();
        let TabDataState::Loaded(ref active_servers) = active_modal.mcps_data else {
            panic!("active modal must still be in Loaded state");
        };
        assert_eq!(
            active_servers[0].status,
            McpServerDisplayStatus::Initializing,
            "active-view agent must not absorb the owning agent's push"
        );
    }

    #[test]
    fn mcp_init_progress_creates_when_none() {
        // When mcp_init_progress is None (no seed), init_progress
        // creates a fresh McpInitProgress.
        let mut app = make_app_with_agent("sess-1");
        assert!(app.agents[&AgentId(0)].session.mcp_init_progress().is_none());

        let notif = make_mcp_init_progress_notif(3, 1);
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(changed);

        let progress = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!(progress.total, 3);
        assert_eq!(progress.connected, 1);
    }

    #[test]
    fn mcp_initialized_clears_progress() {
        // grow/mcp_initialized must set mcp_init_progress to None.
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.update_mcp_init_progress(3, 3);

        let notif = make_mcp_initialized_notif("sess-1");
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(changed);
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_none(),
            "mcp_initialized must clear mcp_init_progress",
        );
    }

    #[test]
    fn mcp_full_lifecycle_seed_to_clear() {
        // Full N-server lifecycle:
        //   seed(0/0) → init_progress(0/3) → init_progress(2/3)
        //   → init_progress(3/3) → mcp_initialized → None
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.update_mcp_init_progress(0, 0);

        // Shell reports real count.
        handle_ext_notification(&make_mcp_init_progress_notif(3, 0), &mut app);
        let p = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!((p.total, p.connected), (3, 0));

        // Incremental progress.
        handle_ext_notification(&make_mcp_init_progress_notif(3, 2), &mut app);
        let p = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!((p.total, p.connected), (3, 2));

        // All connected.
        handle_ext_notification(&make_mcp_init_progress_notif(3, 3), &mut app);
        let p = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!((p.total, p.connected), (3, 3));

        // mcp_initialized clears everything.
        handle_ext_notification(&make_mcp_initialized_notif("sess-1"), &mut app);
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_none(),
            "mcp_initialized must clear progress after full lifecycle",
        );
    }

    #[test]
    fn mcp_zero_server_lifecycle() {
        // 0-server lifecycle (the bug scenario):
        //   seed(0/0) → init_progress(0/0) → mcp_initialized → None
        // Previously mcp_initialized was never sent for 0 servers,
        // leaving a stuck progress indicator.
        let mut app = make_app_with_agent("sess-1");
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        agent.session.update_mcp_init_progress(0, 0);

        // Shell sends 0/0 for the 0-server case.
        handle_ext_notification(&make_mcp_init_progress_notif(0, 0), &mut app);
        let p = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!((p.total, p.connected), (0, 0));

        // Shell now sends mcp_initialized (root-cause fix).
        handle_ext_notification(&make_mcp_initialized_notif("sess-1"), &mut app);
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_none(),
            "0-server mcp_initialized must clear progress",
        );
    }

    #[test]
    fn mcp_init_progress_routes_to_background_session() {
        // init_progress carrying a background session's sessionId must update
        // *that* agent's indicator, not the foregrounded one, and must not
        // force a redraw (the background spinner isn't visible).
        let mut app = make_app_with_agent("sess-A");
        app.agents.insert(AgentId(1), make_agent(Some("sess-B")));

        let notif = make_mcp_init_progress_notif_for(4, 1, "sess-B");
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            !changed,
            "background-session progress must not force a redraw"
        );

        let bg = app.agents[&AgentId(1)].session.mcp_init_progress().unwrap();
        assert_eq!((bg.total, bg.connected), (4, 1));
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_none(),
            "foreground agent must be untouched by a background session's progress",
        );
    }

    #[test]
    fn mcp_initialized_routes_to_background_session() {
        // mcp_initialized for a background session must clear *that* agent's
        // indicator while leaving the foreground agent's intact. Previously
        // the clear was applied to whichever agent was active, so a
        // background agent's spinner could stick forever.
        let mut app = make_app_with_agent("sess-A");
        app.agents.insert(AgentId(1), make_agent(Some("sess-B")));
        for id in [AgentId(0), AgentId(1)] {
        app.agents
            .get_mut(&id)
            .unwrap()
            .session
            .update_mcp_init_progress(2, 0);
        }

        let notif = make_mcp_initialized_notif_for("sess-B");
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(
            !changed,
            "clearing a background spinner must not force a redraw",
        );
        assert!(
            app.agents[&AgentId(1)].session.mcp_init_progress().is_none(),
            "background session's spinner must be cleared",
        );
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_some(),
            "foreground agent's spinner must NOT be cleared by another session",
        );
    }

    #[test]
    fn mcp_initialized_unknown_session_is_dropped() {
        // An mcp_initialized for a session that matches no agent must not
        // clear anyone's indicator (no misrouting to the active agent).
        let mut app = make_app_with_agent("sess-A");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .update_mcp_init_progress(1, 0);

        let notif = make_mcp_initialized_notif_for("sess-unknown");
        let changed = handle_ext_notification(&notif, &mut app);
        assert!(!changed);
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_some(),
            "unknown-session mcp_initialized must not clear the active agent",
        );
    }

    #[test]
    fn mcp_lifecycle_notif_for_subagent_session_is_dropped() {
        // A subagent runs its own MCP init, emitting init_progress /
        // mcp_initialized under the *child* session id. Those must NOT write to
        // or clear the parent agent's mcp_init_progress — it's a per-root-agent
        // indicator with no subagent slot, so a subagent's init must not
        // clobber the parent's spinner.
        let mut app = make_app_with_agent("sess-A");
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .update_mcp_init_progress(2, 1);
        // Register a subagent child view keyed by the child session id.
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .subagent_views
            .insert(
                "child-sess".to_string(),
                Box::new(make_agent(Some("child-sess"))),
            );

        // init_progress for the child session must leave the parent untouched.
        let changed = handle_ext_notification(
            &make_mcp_init_progress_notif_for(5, 0, "child-sess"),
            &mut app,
        );
        assert!(
            !changed,
            "subagent init_progress must not redraw the parent"
        );
        let p = app.agents[&AgentId(0)].session.mcp_init_progress().unwrap();
        assert_eq!(
            (p.total, p.connected),
            (2, 1),
            "parent spinner must be untouched by a subagent's init",
        );

        // mcp_initialized for the child session must not clear the parent.
        let changed =
            handle_ext_notification(&make_mcp_initialized_notif_for("child-sess"), &mut app);
        assert!(!changed);
        assert!(
            app.agents[&AgentId(0)].session.mcp_init_progress().is_some(),
            "subagent mcp_initialized must not clear the parent's spinner",
        );
    }

    #[test]
    fn server_status_handler_noop_when_modal_closed_background() {
        use shell::extensions::mcp::McpServerStatus;
        let mut app = make_app_two_agents();
        // Owner is background and has NO modal open. server_status
        // must be a silent no-op (no Effect scheduling, no redraw).
        let notif = make_server_status_notif("sess-owner", "alpha", McpServerStatus::Ready, None);
        let redraw = handle_mcp_server_status(&notif, &mut app);
        assert!(!redraw, "closed-modal cheap path must not request a redraw");
        assert!(
            app.pending_effects.is_empty(),
            "closed-modal cheap path must not schedule any effects"
        );
        assert!(
            app.agents
                .get(&AgentId(0))
                .unwrap()
                .extensions_modal
                .is_none(),
            "owner modal must remain closed"
        );
    }

    /// Pin the cheap-path semantics for the FOREGROUND case too.
    /// Without this, the background case passes trivially regardless
    /// of how the cheap path is gated on `is_active`.
    #[test]
    fn server_status_handler_noop_when_modal_closed_foreground() {
        use shell::extensions::mcp::McpServerStatus;
        let mut app = make_app_two_agents();
        // Foreground = agent 1 (sess-active). Send a push targeting
        // the foregrounded agent, no modal open.
        let notif = make_server_status_notif("sess-active", "alpha", McpServerStatus::Ready, None);
        let redraw = handle_mcp_server_status(&notif, &mut app);
        assert!(
            !redraw,
            "foreground + closed-modal must still be a no-op (no row → no redraw)"
        );
        assert!(
            app.pending_effects.is_empty(),
            "server_status NEVER schedules an effect (push is per-row, not list-refetch)"
        );
    }

    /// Pin the cheap-path semantics when the modal is open but the
    /// data is still loading. The `TabDataState::Loaded`
    /// gate must early-return without panicking.
    #[test]
    fn server_status_handler_noop_when_modal_data_still_loading() {
        use crate::views::extensions_modal::{ExtensionsModalState, ExtensionsTab, TabDataState};
        use shell::extensions::mcp::McpServerStatus;
        let mut app = make_app_two_agents();
        {
            let owner = app.agents.get_mut(&AgentId(0)).unwrap();
            let mut modal = ExtensionsModalState::new(ExtensionsTab::McpServers);
            modal.mcps_data = TabDataState::Loading;
            owner.extensions_modal = Some(modal);
        }
        let notif = make_server_status_notif("sess-owner", "alpha", McpServerStatus::Ready, None);
        let redraw = handle_mcp_server_status(&notif, &mut app);
        assert!(!redraw, "Loading state must skip the patch");
        assert!(app.pending_effects.is_empty());
    }

    /// A malformed `status` field must NOT silently coerce to
    /// `Unavailable`. The whole payload must fail to parse and
    /// warn-log instead.
    #[test]
    fn server_status_malformed_status_does_not_silently_repaint() {
        use crate::views::extensions_modal::TabDataState;
        use crate::views::mcps_modal::McpServerDisplayStatus;
        let mut app = make_app_two_agents();
        seed_owner_agent_with_open_modal(&mut app);
        // Send a payload with the `status` field missing entirely —
        // a defaulting decoder would silently coerce this to Unavailable.
        let payload = serde_json::json!({
            "sessionId": "sess-owner",
            "name": "alpha",
            "source": "local",
            "reason": "initialized",
            // NB: no `status`.
        });
        let raw = serde_json::value::to_raw_value(&payload).unwrap();
        let notif = acp::ExtNotification::new("grow/mcp/server_status", raw.into());
        let redraw = handle_mcp_server_status(&notif, &mut app);
        assert!(!redraw, "malformed payload must not request a redraw");

        let modal = app
            .agents
            .get(&AgentId(0))
            .unwrap()
            .extensions_modal
            .as_ref()
            .unwrap();
        let TabDataState::Loaded(ref servers) = modal.mcps_data else {
            panic!("modal still Loaded");
        };
        assert_eq!(
            servers[0].status,
            McpServerDisplayStatus::Initializing,
            "malformed status must NOT silently map to Unavailable"
        );
    }

    /// Pin that the pager deserializes against the *shell's*
    /// `McpServerStatus` enum, so a future new variant doesn't need a
    /// pager change to be recognized. Round-trip through
    /// `serde_json::to_string` of the shell type itself.
    #[test]
    fn server_status_round_trips_shell_canonical_type() {
        use shell::extensions::mcp::{
            McpServerStatus, McpServerStatusPayload, McpServerStatusReason,
        };
        let payload = McpServerStatusPayload {
            session_id: "s".into(),
            name: "alpha".into(),
            status: McpServerStatus::Ready,
            reason: McpServerStatusReason::Initialized,
            detail: None,
            tools: None,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let roundtripped: McpServerStatusPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(payload, roundtripped);
        assert!(json.contains("\"ready\""));
    }


    /// `mcp_initialized` wire shape `{ sessionId, mcpToolCount, elapsedMs }`
    /// — sessionId routing applies; the matched agent's
    /// `mcp_init_progress` overlay must be cleared.
    #[test]
    fn mcp_initialized_clears_init_progress_on_owner() {
        let mut app = make_app_two_agents();
        // Seed init progress on the OWNER (agent 0) and on the
        // active view (agent 1). The push must clear only the
        // owner's overlay.
        {
            let owner = app.agents.get_mut(&AgentId(0)).unwrap();
            owner.session.update_mcp_init_progress(5, 3);
        }
        {
            let active = app.agents.get_mut(&AgentId(1)).unwrap();
            active.session.update_mcp_init_progress(5, 3);
        }

        let notif = make_mcp_initialized_notif_for("sess-owner");
        let _ = handle_mcp_initialized(&notif, &mut app);

        assert!(
            app.agents
                .get(&AgentId(0))
                .unwrap()
                .session
                .mcp_init_progress()
                .is_none(),
            "owner's init progress overlay must be cleared"
        );
        assert!(
            app.agents
                .get(&AgentId(1))
                .unwrap()
                .session
                .mcp_init_progress()
                .is_some(),
            "active-view agent's init progress must NOT be cleared by a background-agent init"
        );
    }

    #[test]
    fn catalog_delta_routes_refresh_to_owning_agent_not_active_view() {
        let mut app = make_app_two_agents();
        seed_owner_agent_with_open_modal(&mut app);
        // Active agent has NO modal — a route-by-active-view path
        // would no-op. Routing by sessionId must schedule a fetch
        // against the OWNER agent (agent 0).
        let notif = make_catalog_refresh_notif("sess-owner", "linear");
        let _ = handle_mcp_server_status(&notif, &mut app);

        assert_eq!(
            app.pending_effects.len(),
            1,
            "catalog delta with sessionId must route to the owner"
        );
        let Effect::FetchMcpsList { agent_id, .. } =
            app.pending_effects.first().expect("effect scheduled")
        else {
            panic!("expected FetchMcpsList");
        };
        assert_eq!(
            *agent_id,
            AgentId(0),
            "catalog delta with sessionId must route to the owner, not the active view"
        );
    }

    #[test]
    fn catalog_delta_without_session_id_is_rejected() {
        let mut app = make_app_two_agents();
        app.agents.get_mut(&AgentId(1)).unwrap().extensions_modal =
            Some(make_mcps_modal_with_servers(Vec::new()));
        let raw = serde_json::value::to_raw_value(
            &serde_json::json!({
                "name": "orphan",
                "status": "ready",
                "reason": "config_changed"
            }),
        )
        .unwrap();
        let notif = acp::ExtNotification::new(
            "grow/mcp/server_status",
            std::sync::Arc::from(raw),
        );

        assert!(!handle_mcp_server_status(&notif, &mut app));
        assert!(app.pending_effects.is_empty());
    }

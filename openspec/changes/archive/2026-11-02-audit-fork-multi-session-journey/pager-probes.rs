// Audit-only probes. Append inside root::dispatch::tests::dashboard; not a production test module.
#[test]
fn audit_fork_journey_stop_child_only_root() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().session.subagent_sessions.insert(
        "audit-child".into(), make_test_subagent("audit-child", "audit-task"));
    open_dashboard(&mut app);
    app.dashboard.as_mut().unwrap().focus_row(crate::views::dashboard::DashboardRowId::TopLevel(id));
    assert_eq!(crate::views::dashboard::classify_top_level(&app.agents[&id]), crate::views::dashboard::RowState::Working);
    let effects = dispatch_dashboard_stop(&mut app);
    assert!(effects.iter().any(|e| matches!(e, Effect::CancelTurn { cancel_subagents: true, .. })),
        "a child-only Working root needs a stop effect, got {effects:?}");
}

#[test]
fn audit_fork_journey_stop_goal_only_root() {
    let mut app = test_app_with_agent();
    let id = AgentId(0);
    app.agents.get_mut(&id).unwrap().session.goal_state = Some(crate::app::session::GoalDisplayState::test_stub());
    open_dashboard(&mut app);
    app.dashboard.as_mut().unwrap().focus_row(crate::views::dashboard::DashboardRowId::TopLevel(id));
    assert_eq!(crate::views::dashboard::classify_top_level(&app.agents[&id]), crate::views::dashboard::RowState::Working);
    let effects = dispatch_dashboard_stop(&mut app);
    assert!(!effects.is_empty(), "a goal-only Working root needs a stop effect");
}

#[test]
fn audit_fork_journey_close_last_local_view() {
    let mut app = test_app_with_agent();
    open_dashboard(&mut app);
    app.dashboard.as_mut().unwrap().attached_agent = Some(AgentId(0));
    dispatch_dashboard_overlay_stop(&mut app);
    assert!(!app.agents.contains_key(&AgentId(0)), "the dashboard/new-agent surface can survive the last local view closing");
}

#[test]
fn audit_fork_journey_close_owned_root_then_quit_with_viewer() {
    let mut app = test_app_with_agent();
    app.agents.get_mut(&AgentId(0)).unwrap().session.attached_as_viewer = false;
    let viewer = insert_second_agent(&mut app);
    app.agents.get_mut(&viewer).unwrap().session.attached_as_viewer = true;
    let closed = dispatch_sessions_confirm_close(&mut app, AgentId(0));
    assert!(closed.iter().all(|e| matches!(e, Effect::UnregisterActiveSession { .. })), "UI close has no backend teardown: {closed:?}");
    assert!(!app.agents.contains_key(&AgentId(0)));
    let effects = dispatch(Action::Quit, &mut app);
    assert!(effects.iter().any(|e| matches!(e, Effect::SuspendForExit { .. })),
        "the backend-owned root still needs suspension after its view closes, got {effects:?}");
}

#[test]
fn audit_fork_journey_closed_session_reappears_as_roster_row() {
    let mut app = test_app_with_agent();
    let closed_sid = app.agents[&AgentId(0)].session.session_id.as_ref().unwrap().to_string();
    insert_second_agent(&mut app);
    app.leader_roster = vec![idle_roster_entry(&closed_sid, "Audit closed session")];
    open_dashboard(&mut app);
    dispatch_sessions_confirm_close(&mut app, AgentId(0));
    app.dashboard.as_mut().unwrap().collapsed_sections.clear();
    let rows = dashboard_row_order(&app);
    assert!(!rows.contains(&crate::views::dashboard::DashboardRowId::Roster { session_id: closed_sid }),
        "a supposedly closed session is still visible through the roster: {rows:?}");
}

#[test]
fn audit_fork_journey_delete_stay_retires_local_binding() {
    let mut app = test_app_with_agent();
    let sid = app.agents[&AgentId(0)].session.session_id.as_ref().unwrap().to_string();
    app.active_view = ActiveView::Welcome;
    dispatch_task_result(TaskResult::DeleteSessionComplete {
        session_id: sid.clone(), after: crate::app::actions::AfterSessionDelete::Stay,
    }, &mut app);
    assert!(!app.agents.values().any(|agent| agent.session.session_id.as_ref().is_some_and(|id| id.0.as_ref() == sid)),
        "successful delete must retire same-session views even when staying on Welcome");
}

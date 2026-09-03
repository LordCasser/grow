use super::*;
use crate::scrollback::block::BlockContent;
use crate::scrollback::blocks::{OtherToolCallBlock, ToolCallBlock};
use crate::scrollback::types::DisplayMode;
use shell::extensions::notification::{UiNotice, UiNoticeCategory, UiNoticeTone};

fn notice(id: &str, subject: &str, message: &str, tone: UiNoticeTone) -> GrowSessionUpdate {
    let audit = shell::coordination::IncomingInquiryAudit {
        source_peer_id: "peer-process".into(),
        source_session_id: "peer".into(),
        source_cwd: "/tmp/work".into(),
        question: "Status?".into(),
        approval: (subject == "inquiry approval").then(|| "approved".into()),
        outcome: (subject == "inquiry completed")
            .then(|| shell::coordination::InquiryOutcome::answered(id, "Working on tests".into())),
    };
    GrowSessionUpdate::UiNotice(UiNotice {
        correlation_id: id.into(),
        category: UiNoticeCategory::Coordination,
        subject: Some(subject.into()),
        description: None,
        message: message.into(),
        tone,
        details: Some(serde_json::to_string(&audit).unwrap()),
    })
}

fn tool_row(app: &AppView, index: usize) -> &OtherToolCallBlock {
    match &app.agents[&AgentId(0)]
        .scrollback
        .entry(index)
        .unwrap()
        .block
    {
        RenderBlock::ToolCall(ToolCallBlock::Other(block)) => block,
        other => panic!("expected an ordinary tool-style row, got {other:?}"),
    }
}

#[test]
fn coordination_live_snapshot_survives_full_and_cursor_reload_finalization() {
    for full_replay in [false, true] {
        let mut app = make_app_with_agent("target");
        let started = notice(
            "one",
            "incoming inquiry",
            "Answering session peer",
            UiNoticeTone::Info,
        );
        handle(
            make_ext_session_notification("target", started.clone()),
            &mut app,
        );
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .begin_session_reload(42);
        if full_replay {
            handle(
                make_replayed_ext_session_notification("target", "start", started.clone()),
                &mut app,
            );
            assert!(!app.agents[&AgentId(0)].scrollback.has_running_entries());
        }
        // The backend publishes this transient snapshot after durable replay.
        handle(make_ext_session_notification("target", started), &mut app);
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert!(agent.finalize_reload_and_maybe_adopt(42, true, None));
        assert_eq!(agent.scrollback.len(), 1);
        let entry = agent.scrollback.entry(0).unwrap();
        assert!(
            entry.is_running,
            "reload cleanup must not finalize a live sideband"
        );
        let RenderBlock::ToolCall(ToolCallBlock::Other(block)) = &entry.block else {
            panic!()
        };
        assert!(block.elapsed_ms.is_none());
        assert!(matches!(agent.session.state, AgentState::Idle));
    }
}

#[test]
fn coordination_unstructured_receipt_is_a_finite_notice_not_an_unowned_running_row() {
    let mut app = make_app_with_agent("target");
    let GrowSessionUpdate::UiNotice(mut receipt) = notice(
        "one",
        "incoming inquiry",
        "Answering session peer",
        UiNoticeTone::Info,
    ) else {
        panic!()
    };
    receipt.details = Some("A historical receipt without a structured identity".into());
    handle(
        make_ext_session_notification("target", GrowSessionUpdate::UiNotice(receipt)),
        &mut app,
    );
    assert!(matches!(
        app.agents[&AgentId(0)].scrollback.entry(0).unwrap().block,
        RenderBlock::Notice(_)
    ));
    assert!(!app.agents[&AgentId(0)].scrollback.has_running_entries());
}

#[test]
fn coordination_source_audit_is_hidden_live_and_on_replay() {
    let mut app = make_app_with_agent("source");
    for subject in ["outgoing inquiry", "outgoing inquiry completed"] {
        let update = notice(
            "inquiry-1",
            subject,
            "Asking session target",
            UiNoticeTone::Info,
        );
        assert!(!handle(
            make_ext_session_notification("source", update.clone()),
            &mut app
        ));
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = true;
        assert!(!handle(
            make_replayed_ext_session_notification("source", subject, update),
            &mut app
        ));
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .session
            .loading_replay = false;
    }
    assert!(app.agents[&AgentId(0)].scrollback.is_empty());
}

#[test]
fn coordination_source_tools_keep_normal_running_rows_and_full_return_values() {
    use tools::implementations::grow_build::coordination::{
        CoordinationInquiryResult, ListActiveSessionsOutput,
    };
    use tools::types::output::ToolOutput;

    let mut app = make_app_with_agent("source");
    for (index, (name, output)) in [
        (
            "list_active_sessions",
            ToolOutput::ListActiveSessions(ListActiveSessionsOutput { sessions: vec![] }),
        ),
        (
            "ask_session",
            ToolOutput::CoordinationInquiry(CoordinationInquiryResult {
                inquiry_id: "inquiry-1".into(),
                status: "answered".into(),
                answer: Some("Working on tests".into()),
                error: None,
            }),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let text = output.to_prompt_format();
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        let meta = NotificationMeta::default();
        agent.session.handle_update(
            acp::SessionUpdate::ToolCall(
                acp::ToolCall::new(acp::ToolCallId::new(name), name)
                    .kind(acp::ToolKind::Other)
                    .status(acp::ToolCallStatus::Pending),
            ),
            &meta,
            &mut agent.scrollback,
        );
        agent.session.handle_update(
            acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
                acp::ToolCallId::new(name),
                acp::ToolCallUpdateFields::new().status(Some(acp::ToolCallStatus::InProgress)),
            )),
            &meta,
            &mut agent.scrollback,
        );
        let entry = agent.scrollback.entry(index).unwrap();
        assert!(entry.is_running);
        let entry_id = entry.id;
        assert!(matches!(
            agent.session.tracker.activity(),
            Some(TurnActivity::ToolRunning { .. })
        ));
        agent.session.handle_update(
            acp::SessionUpdate::ToolCallUpdate(
                shell::session::acp_conversion::acp_tool_update(&output, name, None, None)
                    .expect("the actual shell conversion must publish this tool result"),
            ),
            &meta,
            &mut agent.scrollback,
        );
        let entry = agent.scrollback.entry(index).unwrap();
        assert_eq!(entry.id, entry_id);
        assert!(!entry.is_running);
        assert!(entry.block.is_foldable());
        assert_eq!(entry.display_mode, DisplayMode::Collapsed);
        assert!(agent.session.tracker.activity().is_none());
        let block = tool_row(&app, index);
        assert_eq!(block.name, name);
        assert_eq!(block.output.as_deref(), Some(text.as_str()));
        assert!(
            block.coordination.is_none(),
            "source is a real tool, not a passive notice"
        );
    }
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 2);
}

#[test]
fn coordination_target_start_approval_and_end_update_one_row_in_place() {
    let mut app = make_app_with_agent("target");
    assert!(handle(
        make_ext_session_notification(
            "target",
            notice(
                "inquiry-1",
                "incoming inquiry",
                "Answering session peer",
                UiNoticeTone::Info,
            )
        ),
        &mut app
    ));
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    let mut appearance = crate::appearance::AppearanceConfig::default();
    appearance.scrollback.scroll.respect_manual_folds = true;
    agent.scrollback.set_appearance(appearance);
    let entry = agent.scrollback.entry(0).unwrap();
    let id = entry.id;
    assert!(entry.is_running);
    assert!(matches!(agent.session.state, AgentState::Idle));
    assert!(
        agent.session.tracker.activity().is_none(),
        "a sideband must not own the primary turn"
    );
    assert_eq!(
        agent.scrollback.last_tool_call_entry_id(),
        None,
        "tool hooks must not attach here"
    );
    agent.scrollback.set_selected(Some(0));
    agent.scrollback.expand_selected();
    assert!(agent.scrollback.get_by_id(id).unwrap().display_mode_pinned);
    agent
        .scrollback
        .push_block(RenderBlock::notice("unrelated later activity"));

    assert!(handle(
        make_ext_session_notification(
            "target",
            notice(
                "inquiry-1",
                "inquiry approval",
                "Answering session peer",
                UiNoticeTone::Success,
            )
        ),
        &mut app
    ));
    let agent = app.agents.get_mut(&AgentId(0)).unwrap();
    agent.session.finish_turn(&mut agent.scrollback);
    assert!(
        agent.scrollback.get_by_id(id).unwrap().is_running,
        "finishing the foreground must not finish the sideband"
    );

    assert!(handle(
        make_ext_session_notification(
            "target",
            notice(
                "inquiry-1",
                "inquiry completed",
                "Answered session peer",
                UiNoticeTone::Success,
            )
        ),
        &mut app
    ));
    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 2);
    let entry = agent.scrollback.entry(0).unwrap();
    assert_eq!(entry.id, id);
    assert!(!entry.is_running);
    assert_eq!(
        entry.display_mode,
        DisplayMode::Expanded,
        "completion must preserve manual expansion"
    );
    assert!(
        !entry.block.is_groupable(),
        "a passive inquiry must remain independently visible"
    );
    let block = tool_row(&app, 0);
    assert_eq!(block.name, "Answered session peer");
    assert!(block.coordination.as_ref().unwrap().terminal);
    let details = block.output.as_deref().unwrap();
    for expected in [
        "Inquiry ID: inquiry-1",
        "Source workspace: /tmp/work",
        "Status?",
        "Working on tests",
    ] {
        assert!(
            details.contains(expected),
            "missing audit detail: {expected}"
        );
    }
}

#[test]
fn coordination_replay_and_late_events_do_not_duplicate_or_resurrect_finished_rows() {
    let mut app = make_app_with_agent("target");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
    let started = notice(
        "inquiry-1",
        "incoming inquiry",
        "Answering session peer",
        UiNoticeTone::Info,
    );
    let completed = notice(
        "inquiry-1",
        "inquiry completed",
        "Answered session peer",
        UiNoticeTone::Success,
    );
    assert!(handle(
        make_replayed_ext_session_notification("target", "start", started.clone()),
        &mut app
    ));
    assert!(
        !app.agents[&AgentId(0)].scrollback.has_running_entries(),
        "historical start alone does not prove liveness"
    );
    assert!(handle(
        make_replayed_ext_session_notification("target", "end", completed.clone()),
        &mut app
    ));
    assert!(!handle(
        make_replayed_ext_session_notification("target", "start-again", started.clone()),
        &mut app
    ));
    assert!(!handle(
        make_replayed_ext_session_notification("target", "end-again", completed.clone()),
        &mut app
    ));
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = false;
    assert!(!handle(
        make_ext_session_notification("target", started),
        &mut app
    ));
    assert!(!handle(
        make_ext_session_notification("target", completed),
        &mut app
    ));
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 1);
    assert!(!app.agents[&AgentId(0)].scrollback.has_running_entries());
    assert_eq!(tool_row(&app, 0).name, "Answered session peer");
}

#[test]
fn coordination_failure_or_cancel_updates_the_original_row() {
    for (title, tone) in [
        ("Failed to answer session peer", UiNoticeTone::Error),
        ("Cancelled answer to session peer", UiNoticeTone::Warning),
        ("Rejected inquiry from session peer", UiNoticeTone::Warning),
    ] {
        let mut app = make_app_with_agent("target");
        handle(
            make_ext_session_notification(
                "target",
                notice(
                    "inquiry-1",
                    "incoming inquiry",
                    "Answering session peer",
                    UiNoticeTone::Info,
                ),
            ),
            &mut app,
        );
        handle(
            make_ext_session_notification(
                "target",
                notice("inquiry-1", "inquiry completed", title, tone),
            ),
            &mut app,
        );
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 1);
        assert!(!app.agents[&AgentId(0)].scrollback.has_running_entries());
        let block = tool_row(&app, 0);
        assert_eq!(block.name, title);
        assert!(!block.is_success());
        assert!(block.is_foldable());
    }
}

#[test]
fn coordination_runtime_health_errors_remain_visible() {
    let mut app = make_app_with_agent("target");
    assert!(handle(
        make_ext_session_notification(
            "target",
            notice(
                "runtime",
                "runtime unavailable",
                "Local coordination is unavailable",
                UiNoticeTone::Error,
            )
        ),
        &mut app
    ));
    assert!(matches!(
        app.agents[&AgentId(0)].scrollback.entry(0).unwrap().block,
        RenderBlock::Notice(_)
    ));
}

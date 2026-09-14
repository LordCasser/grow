use super::*;
use crate::scrollback::block::BlockContent;
use crate::scrollback::blocks::{OtherToolCallBlock, ToolCallBlock};
use crate::scrollback::types::DisplayMode;
use shell::extensions::notification::{
    ParentMessageNotice, UiNotice, UiNoticeCategory, UiNoticeTone,
};

fn notice(id: &str, subject: &str, message: &str, tone: UiNoticeTone) -> GrowSessionUpdate {
    let audit = shell::coordination::IncomingInquiryAudit {
        direction: shell::coordination::InquiryDirection::Peer,
        source_peer_id: "peer-process".into(),
        source_session_id: "peer".into(),
        source_cwd: "/tmp/work".into(),
        delegated_subagent_task_name: None,
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

fn parent_message_notice(receipt_id: &str, message: Option<&str>) -> GrowSessionUpdate {
    let data = ParentMessageNotice {
        parent_session_id: "parent-session".into(),
        message_id: "message-1".into(),
        interrupt: true,
        message: message.map(str::to_owned),
    };
    GrowSessionUpdate::UiNotice(UiNotice {
        correlation_id: receipt_id.into(),
        category: UiNoticeCategory::Coordination,
        subject: Some(ParentMessageNotice::SUBJECT.into()),
        description: None,
        message: if data.message.is_some() {
            "Parent guidance received".into()
        } else {
            "Parent guidance received · Message body could not be recovered".into()
        },
        tone: if data.message.is_some() {
            UiNoticeTone::Info
        } else {
            UiNoticeTone::Warning
        },
        details: Some(serde_json::to_string(&data).unwrap()),
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
            "ask_parent",
            ToolOutput::AgentInteraction(
                tools::implementations::grow_build::task::interaction::AgentInteractionOutput {
                    id: "child:question".into(),
                    status: "answered".into(),
                    answer: Some("Use the existing interface".into()),
                    error: None,
                    subagent_task_name: None,
                    target_session_id: None,
                },
            ),
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
    assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 3);
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
fn coordination_target_uses_persisted_subagent_task_title() {
    let mut app = make_app_with_agent("target");
    let GrowSessionUpdate::UiNotice(mut update) = notice(
        "inquiry-1",
        "inquiry completed",
        "Answered subagent TS registry workload presentation",
        UiNoticeTone::Success,
    ) else {
        panic!()
    };
    let mut audit: shell::coordination::IncomingInquiryAudit =
        serde_json::from_str(update.details.as_deref().unwrap()).unwrap();
    audit.delegated_subagent_task_name = Some("TS registry workload presentation".into());
    audit.direction = shell::coordination::InquiryDirection::ChildToParent;
    update.details = Some(serde_json::to_string(&audit).unwrap());

    assert!(handle(
        make_ext_session_notification("target", GrowSessionUpdate::UiNotice(update)),
        &mut app
    ));
    let block = tool_row(&app, 0);
    assert_eq!(
        block.name,
        "Answered subagent TS registry workload presentation"
    );
    assert!(block.output.as_deref().is_some_and(|details| {
        details.contains("Subagent task: TS registry workload presentation")
    }));
}

#[test]
fn coordination_child_receiving_parent_inquiry_names_parent() {
    let mut app = make_app_with_agent("child");
    let GrowSessionUpdate::UiNotice(mut update) = notice(
        "inquiry-1",
        "inquiry completed",
        "Answered parent agent",
        UiNoticeTone::Success,
    ) else {
        panic!()
    };
    let mut audit: shell::coordination::IncomingInquiryAudit =
        serde_json::from_str(update.details.as_deref().unwrap()).unwrap();
    audit.direction = shell::coordination::InquiryDirection::ParentToChild;
    audit.delegated_subagent_task_name = Some("TS registry workload presentation".into());
    update.details = Some(serde_json::to_string(&audit).unwrap());

    assert!(handle(
        make_ext_session_notification("child", GrowSessionUpdate::UiNotice(update)),
        &mut app
    ));
    let block = tool_row(&app, 0);
    assert_eq!(block.name, "Answered parent agent");
    let details = block.output.as_deref().unwrap();
    assert!(details.contains("Direction: parent_to_child"));
    assert!(details.contains("Subagent task: TS registry workload presentation"));
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

#[test]
fn delegated_question_updates_one_primary_view_row() {
    let mut app = make_app_with_agent("parent");
    for subject in ["incoming inquiry", "inquiry approval", "inquiry completed"] {
        let GrowSessionUpdate::UiNotice(mut update) =
            notice("child:call", subject, "Answering child", UiNoticeTone::Info)
        else {
            panic!()
        };
        let mut audit: shell::coordination::IncomingInquiryAudit =
            serde_json::from_str(update.details.as_ref().unwrap()).unwrap();
        audit.direction = shell::coordination::InquiryDirection::ChildToParent;
        audit.source_peer_id = "local-delegation".into();
        audit.source_session_id = "child".into();
        audit.delegated_subagent_task_name = Some("TS registry workload presentation".into());
        update.message = if subject == "inquiry completed" {
            "Answered subagent TS registry workload presentation".into()
        } else {
            "Answering subagent TS registry workload presentation".into()
        };
        update.details = Some(serde_json::to_string(&audit).unwrap());
        handle(
            make_ext_session_notification("parent", GrowSessionUpdate::UiNotice(update)),
            &mut app,
        );
        assert_eq!(app.agents[&AgentId(0)].scrollback.len(), 1);
        assert!(app.agents[&AgentId(0)].session.tracker.activity().is_none());
    }
    assert!(
        !app.agents[&AgentId(0)]
            .scrollback
            .entry(0)
            .unwrap()
            .is_running
    );
    assert!(
        tool_row(&app, 0)
            .output
            .as_ref()
            .unwrap()
            .contains("Working on tests")
    );
    assert_eq!(
        tool_row(&app, 0).name,
        "Answered subagent TS registry workload presentation"
    );
}

#[test]
fn parent_message_live_and_replay_share_receipt_identity() {
    let mut app = make_app_with_agent("child");
    let update = parent_message_notice("receipt-1", Some("first line\nsecond line\nthird line"));
    assert!(handle(
        make_ext_session_notification("child", update.clone()),
        &mut app,
    ));
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
    assert!(handle(
        make_replayed_ext_session_notification("child", "transport-event-9", update),
        &mut app,
    ));

    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 1);
    let RenderBlock::Notice(notice) = &agent.scrollback.entry(0).unwrap().block else {
        panic!("expected parent message NoticeBlock");
    };
    assert_eq!(notice.event_id.as_deref(), Some("parent-message:receipt-1"));
    assert!(
        notice
            .details
            .as_deref()
            .unwrap()
            .contains("Receipt ID: receipt-1")
    );
    assert!(!notice.details.as_deref().unwrap().contains("Inquiry ID:"));
}

#[test]
fn parent_message_full_and_cursor_reload_keep_one_finite_notice() {
    for full_replay in [false, true] {
        let mut app = make_app_with_agent("child");
        let update = parent_message_notice("receipt-reload", Some("Keep the original interface"));
        handle(
            make_ext_session_notification("child", update.clone()),
            &mut app,
        );
        app.agents
            .get_mut(&AgentId(0))
            .unwrap()
            .begin_session_reload(43);
        if full_replay {
            handle(
                make_replayed_ext_session_notification("child", "child-10", update.clone()),
                &mut app,
            );
        }
        handle(make_ext_session_notification("child", update), &mut app);
        let agent = app.agents.get_mut(&AgentId(0)).unwrap();
        assert!(agent.finalize_reload_and_maybe_adopt(43, true, None));
        assert_eq!(agent.scrollback.len(), 1);
        assert!(!agent.scrollback.has_running_entries());
        assert!(
            matches!(&agent.scrollback.entry(0).unwrap().block, RenderBlock::Notice(notice)
            if notice.event_id.as_deref() == Some("parent-message:receipt-reload"))
        );
        assert!(agent.session.tracker.activity().is_none());
    }
}

#[test]
fn parent_message_routes_to_nested_child_without_switching_view() {
    let mut app = make_app_with_agent("root-session");
    for (parent, child) in [
        ("root-session", "child-session"),
        ("child-session", "grandchild-session"),
    ] {
        handle(
            make_ext_session_notification(parent, test_subagent_spawned(parent, child)),
            &mut app,
        );
    }
    let GrowSessionUpdate::UiNotice(mut notice) =
        parent_message_notice("nested-receipt", Some("Nested guidance"))
    else {
        unreachable!()
    };
    let mut data = ParentMessageNotice::from_notice(&notice).unwrap();
    data.parent_session_id = "child-session".into();
    notice.details = Some(serde_json::to_string(&data).unwrap());
    handle(
        make_ext_session_notification("grandchild-session", GrowSessionUpdate::UiNotice(notice)),
        &mut app,
    );
    let root = &app.agents[&AgentId(0)];
    let child = &root.subagent_views["grandchild-session"];
    assert!(root.active_subagent.is_none());
    assert!((0..child.scrollback.len()).any(|index| matches!(&child.scrollback.entry(index).unwrap().block,
        RenderBlock::Notice(notice) if notice.event_id.as_deref() == Some("parent-message:nested-receipt"))));
    assert!(!(0..root.scrollback.len()).any(|index| matches!(&root.scrollback.entry(index).unwrap().block,
        RenderBlock::Notice(notice) if notice.event_id.as_deref() == Some("parent-message:nested-receipt"))));
}

#[test]
fn parent_message_routes_to_existing_child_view_and_handles_missing_body() {
    let mut app = make_app_with_parent_and_child("parent", "child");
    assert!(handle(
        make_ext_session_notification("child", parent_message_notice("receipt-2", None),),
        &mut app,
    ));

    let parent = &app.agents[&AgentId(0)];
    assert_eq!(parent.scrollback.len(), 0);
    let child = parent.subagent_views.get("child").expect("child view");
    assert_eq!(child.scrollback.len(), 1);
    let RenderBlock::Notice(notice) = &child.scrollback.entry(0).unwrap().block else {
        panic!("expected missing-body NoticeBlock");
    };
    assert_eq!(notice.event_id.as_deref(), Some("parent-message:receipt-2"));
    assert!(
        notice
            .details
            .as_deref()
            .unwrap()
            .contains("Message body could not be recovered")
    );
}

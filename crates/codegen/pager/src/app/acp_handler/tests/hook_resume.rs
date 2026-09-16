use super::*;

use crate::app::agent_view::AgentView;
use crate::app::session::AgentState;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::tool::ToolCallBlock;
use crate::scrollback::entry::ScrollbackEntry;
use crate::scrollback::types::DisplayMode;
use shell::extensions::notification::{HookRunEntryDto, HookRunStatusDto};

fn acp_message(session_id: &str, update: acp::SessionUpdate, replay: bool) -> AcpClientMessage {
    let (response_tx, _response_rx) = tokio::sync::oneshot::channel();
    let mut request = acp::SessionNotification::new(acp::SessionId::new(session_id), update);
    if replay {
        request = request.meta(serde_json::json!({ "isReplay": true }).as_object().cloned());
    }
    AcpClientMessage::SessionNotification(acp_transport::AcpArgs {
        request,
        response_tx,
    })
}

fn tool_start(
    session_id: &str,
    id: &str,
    kind: acp::ToolKind,
    title: &str,
    replay: bool,
) -> AcpClientMessage {
    let tool = acp::ToolCall::new(acp::ToolCallId::new(id), title.to_string())
        .kind(kind)
        .status(acp::ToolCallStatus::Pending)
        .content(vec![])
        .locations(vec![]);
    acp_message(session_id, acp::SessionUpdate::ToolCall(tool), replay)
}

fn tool_precompleted(
    session_id: &str,
    id: &str,
    kind: acp::ToolKind,
    title: &str,
    replay: bool,
) -> AcpClientMessage {
    let tool = acp::ToolCall::new(acp::ToolCallId::new(id), title.to_string())
        .kind(kind)
        .status(acp::ToolCallStatus::Completed)
        .content(vec![])
        .locations(vec![]);
    acp_message(session_id, acp::SessionUpdate::ToolCall(tool), replay)
}

fn tool_complete(
    session_id: &str,
    id: &str,
    kind: acp::ToolKind,
    title: &str,
    raw_input: Option<serde_json::Value>,
    content: Option<Vec<acp::ToolCallContent>>,
    replay: bool,
) -> AcpClientMessage {
    let mut fields = acp::ToolCallUpdateFields::new()
        .kind(Some(kind))
        .title(Some(title.to_string()))
        .status(Some(acp::ToolCallStatus::Completed));
    if let Some(raw_input) = raw_input {
        fields = fields.raw_input(Some(raw_input));
    }
    if let Some(content) = content {
        fields = fields.content(Some(content));
    }
    acp_message(
        session_id,
        acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
            acp::ToolCallId::new(id),
            fields,
        )),
        replay,
    )
}

fn hook_run(name: &str, status: HookRunStatusDto, output: Option<&str>) -> HookRunEntryDto {
    HookRunEntryDto {
        name: name.to_string(),
        status,
        output: output.map(str::to_string),
    }
}

fn hook_update(
    occurrence_id: &str,
    event_name: &str,
    tool_call_id: Option<&str>,
    is_snapshot: bool,
    runs: Vec<HookRunEntryDto>,
    annotations: Vec<&str>,
) -> GrowSessionUpdate {
    GrowSessionUpdate::HookExecution {
        occurrence_id: occurrence_id.to_string(),
        event_name: event_name.to_string(),
        tool_call_id: tool_call_id.map(str::to_string),
        is_snapshot,
        tool_name: None,
        prompt_id: None,
        runs,
        annotations: annotations.into_iter().map(str::to_string).collect(),
    }
}

fn lifecycle_count(agent: &AgentView) -> usize {
    (0..agent.scrollback.len())
        .filter(|index| {
            matches!(
                agent.scrollback.entry(*index).map(|entry| &entry.block),
                Some(RenderBlock::ToolCall(ToolCallBlock::Lifecycle(_)))
            )
        })
        .count()
}

fn hook_text(entry: &ScrollbackEntry, mode: DisplayMode) -> String {
    let context = entry.context_with_mode(
        160,
        mode,
        &crate::appearance::AppearanceConfig::default(),
        None,
    );
    let output = entry.output_with_hooks(&context);
    let mut text = String::new();
    for line in output.lines {
        for span in line.content.spans {
            text.push_str(span.content.as_ref());
        }
        text.push('\n');
    }
    text
}

#[test]
fn completed_replay_tools_accept_late_snapshots_without_tail_rows() {
    let mut app = make_app_with_agent("hook-replay");
    let agent_id = AgentId(0);
    app.agents
        .get_mut(&agent_id)
        .unwrap()
        .session
        .loading_replay = true;

    const TOOL_COUNT: usize = 64;
    for index in 0..TOOL_COUNT {
        let id = format!("replay-tool-{index}");
        handle(
            tool_precompleted("hook-replay", &id, acp::ToolKind::Other, &id, true),
            &mut app,
        );
    }
    app.agents
        .get_mut(&agent_id)
        .unwrap()
        .session
        .loading_replay = false;

    for index in 0..TOOL_COUNT {
        let id = format!("replay-tool-{index}");
        handle(
            make_ext_session_notification(
                "hook-replay",
                hook_update(
                    &format!("replay-pre-{index}"),
                    "pre_tool_use",
                    Some(&id),
                    true,
                    vec![hook_run(
                        "replay-pre-hook",
                        HookRunStatusDto::Success { elapsed_ms: 1 },
                        None,
                    )],
                    vec![],
                ),
            ),
            &mut app,
        );
        handle(
            make_ext_session_notification(
                "hook-replay",
                hook_update(
                    &format!("replay-post-{index}"),
                    "post_tool_use",
                    Some(&id),
                    true,
                    vec![hook_run(
                        "replay-post-hook",
                        HookRunStatusDto::Success { elapsed_ms: 2 },
                        None,
                    )],
                    vec![],
                ),
            ),
            &mut app,
        );
    }

    let agent = &app.agents[&agent_id];
    assert_eq!(agent.scrollback.len(), TOOL_COUNT);
    assert_eq!(
        lifecycle_count(agent),
        0,
        "anchored snapshots must not make tail rows"
    );
    for index in 0..TOOL_COUNT {
        let entry = agent.scrollback.entry(index).expect("replayed tool entry");
        let data = entry.hook_data.as_ref().expect("both hook phases attach");
        assert_eq!(data.pre_hooks.len(), 1);
        assert_eq!(data.post_hooks.len(), 1);
    }
}

#[test]
fn live_parallel_hooks_keep_exact_ownership_and_ignore_snapshot_duplicate() {
    let mut app = make_app_with_agent("hook-live");
    for id in ["live-a", "live-b"] {
        handle(
            tool_start("hook-live", id, acp::ToolKind::Other, id, false),
            &mut app,
        );
    }
    for id in ["live-b", "live-a"] {
        handle(
            tool_complete("hook-live", id, acp::ToolKind::Other, id, None, None, false),
            &mut app,
        );
    }

    for (occurrence, event, id, name) in [
        ("live-a-pre", "pre_tool_use", "live-a", "pre-a"),
        ("live-b-pre", "pre_tool_use", "live-b", "pre-b"),
        ("live-b-post", "post_tool_use", "live-b", "post-b"),
        ("live-a-post", "post_tool_use", "live-a", "post-a"),
    ] {
        handle(
            make_ext_session_notification(
                "hook-live",
                hook_update(
                    occurrence,
                    event,
                    Some(id),
                    false,
                    vec![hook_run(
                        name,
                        HookRunStatusDto::Success { elapsed_ms: 1 },
                        None,
                    )],
                    vec![],
                ),
            ),
            &mut app,
        );
    }
    handle(
        make_ext_session_notification(
            "hook-live",
            hook_update(
                "live-a-post",
                "post_tool_use",
                Some("live-a"),
                true,
                vec![hook_run(
                    "duplicate-a",
                    HookRunStatusDto::Failed {
                        error: "must be ignored".into(),
                        elapsed_ms: 99,
                    },
                    None,
                )],
                vec![],
            ),
        ),
        &mut app,
    );

    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 2);
    assert_eq!(lifecycle_count(agent), 0);
    for (index, expected) in [(0, ["pre-a", "post-a"]), (1, ["pre-b", "post-b"])] {
        let entry = agent.scrollback.entry(index).unwrap();
        let data = entry.hook_data.as_ref().unwrap();
        let names: Vec<_> = data
            .pre_hooks
            .iter()
            .chain(data.post_hooks.iter())
            .map(|run| run.name.as_str())
            .collect();
        assert_eq!(names, expected.to_vec());
    }
}

fn edit_content(path: &str, line: usize) -> Vec<acp::ToolCallContent> {
    vec![acp::ToolCallContent::Diff(
        acp::Diff::new(path, format!("new-{line}"))
            .old_text(Some(format!("old-{line}")))
            .meta(
                serde_json::json!({ "old_line": line, "new_line": line })
                    .as_object()
                    .cloned(),
            ),
    )]
}

#[test]
fn merged_edits_retain_each_same_phase_hook_occurrence() {
    let mut app = make_app_with_agent("hook-edit");
    let agent_id = AgentId(0);
    for id in ["edit-a", "edit-b"] {
        handle(
            tool_start(
                "hook-edit",
                id,
                acp::ToolKind::Edit,
                "search_replace",
                false,
            ),
            &mut app,
        );
    }
    for (id, line) in [("edit-b", 40), ("edit-a", 5)] {
        handle(
            tool_complete(
                "hook-edit",
                id,
                acp::ToolKind::Edit,
                "foo.rs",
                Some(serde_json::json!({ "file_path": "foo.rs" })),
                Some(edit_content("foo.rs", line)),
                false,
            ),
            &mut app,
        );
    }

    // The mapping survives a turn fence; a delayed snapshot still belongs to
    // the completed row instead of falling back to the latest tool.
    {
        let agent = app.agents.get_mut(&agent_id).unwrap();
        agent.session.tracker.finish_turn(&mut agent.scrollback);
    }
    for (id, phase, occurrence) in [
        ("edit-a", "pre_tool_use", "edit-a-pre"),
        ("edit-b", "pre_tool_use", "edit-b-pre"),
        ("edit-a", "post_tool_use", "edit-a-post"),
        ("edit-b", "post_tool_use", "edit-b-post"),
    ] {
        handle(
            make_ext_session_notification(
                "hook-edit",
                hook_update(
                    occurrence,
                    phase,
                    Some(id),
                    true,
                    vec![hook_run(
                        occurrence,
                        HookRunStatusDto::Success { elapsed_ms: 1 },
                        None,
                    )],
                    vec![],
                ),
            ),
            &mut app,
        );
    }

    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 1, "same-file edits should coalesce");
    let entry = agent.scrollback.entry(0).unwrap();
    let RenderBlock::ToolCall(ToolCallBlock::Edit(edit)) = &entry.block else {
        panic!("expected merged Edit block")
    };
    assert_eq!(edit.edit_count, 2);
    let data = entry.hook_data.as_ref().unwrap();
    assert_eq!(
        data.pre_hooks
            .iter()
            .map(|run| run.name.as_str())
            .collect::<Vec<_>>(),
        vec!["edit-a-pre", "edit-b-pre"],
    );
    assert_eq!(
        data.post_hooks
            .iter()
            .map(|run| run.name.as_str())
            .collect::<Vec<_>>(),
        vec!["edit-a-post", "edit-b-post"],
    );
}

#[test]
fn restored_lifecycle_snapshots_are_one_compact_entry_and_keep_details() {
    let mut app = make_app_with_agent("hook-history");
    const SNAPSHOT_COUNT: usize = 1300;
    for index in 0..SNAPSHOT_COUNT {
        let failed = index == SNAPSHOT_COUNT - 1;
        handle(
            make_ext_session_notification(
                "hook-history",
                hook_update(
                    &format!("history-{index}"),
                    "session_start",
                    None,
                    true,
                    vec![hook_run(
                        &format!("history-hook-{index}"),
                        if failed {
                            HookRunStatusDto::Failed {
                                error: "history failure detail".into(),
                                elapsed_ms: 7,
                            }
                        } else {
                            HookRunStatusDto::Success { elapsed_ms: 1 }
                        },
                        failed.then_some("history output detail"),
                    )],
                    if failed {
                        vec!["history annotation detail"]
                    } else {
                        vec![]
                    },
                ),
            ),
            &mut app,
        );
    }

    let agent_id = AgentId(0);
    app.agents.get_mut(&agent_id).unwrap().session.state = AgentState::TurnRunning;
    handle(
        make_ext_session_notification(
            "hook-history",
            hook_update(
                "history-stop",
                "stop",
                None,
                true,
                vec![hook_run(
                    "stop-hook",
                    HookRunStatusDto::Failed {
                        error: "old stop failure".into(),
                        elapsed_ms: 3,
                    },
                    None,
                )],
                vec![],
            ),
        ),
        &mut app,
    );

    let agent = &app.agents[&agent_id];
    assert_eq!(agent.scrollback.len(), 1);
    assert_eq!(lifecycle_count(agent), 1);
    assert!(
        agent.pending_stop_hooks.is_none(),
        "historical stop cannot enter live stash"
    );
    let entry = agent.scrollback.entry(0).unwrap();
    let data = entry.hook_data.as_ref().unwrap();
    assert_eq!(data.lifecycle.len(), SNAPSHOT_COUNT + 1);

    let collapsed = hook_text(entry, DisplayMode::Collapsed);
    assert_eq!(
        collapsed.lines().count(),
        1,
        "collapsed history stays compact"
    );
    assert!(collapsed.contains("Restored hooks"));
    assert!(collapsed.contains("[hooks: 1299/2]"));

    let expanded = hook_text(entry, DisplayMode::Expanded);
    assert!(expanded.contains("history-1299"));
    assert!(expanded.contains("history-hook-1299"));
    assert!(expanded.contains("history failure detail"));
    assert!(expanded.contains("history output detail"));
    assert!(expanded.contains("history-1299: history annotation detail"));
}

#[test]
fn snapshot_hooks_route_through_child_view() {
    let mut app = make_app_with_parent_and_child("hook-parent", "hook-child");
    handle(
        make_ext_session_notification(
            "hook-child",
            hook_update(
                "child-history",
                "session_start",
                None,
                true,
                vec![hook_run(
                    "child-hook",
                    HookRunStatusDto::Success { elapsed_ms: 1 },
                    None,
                )],
                vec!["child annotation"],
            ),
        ),
        &mut app,
    );

    let parent = &app.agents[&AgentId(0)];
    assert_eq!(parent.scrollback.len(), 0);
    let child = parent.subagent_views.get("hook-child").unwrap();
    assert_eq!(child.scrollback.len(), 1);
    assert_eq!(lifecycle_count(child), 1);
    assert!(child.scrollback.entry(0).unwrap().hook_data.is_some());
}

#[test]
fn live_hook_before_tool_call_waits_for_exact_owner() {
    let mut app = make_app_with_agent("hook-overtake");
    // A resident actor can still execute live work while a client reconnects.
    // The snapshot flag, not the view's loading state, identifies history.
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
    handle(
        tool_start(
            "hook-overtake",
            "old-tool",
            acp::ToolKind::Other,
            "old-tool",
            false,
        ),
        &mut app,
    );
    handle(
        tool_complete(
            "hook-overtake",
            "old-tool",
            acp::ToolKind::Other,
            "old-tool",
            None,
            None,
            false,
        ),
        &mut app,
    );

    handle(
        make_ext_session_notification(
            "hook-overtake",
            hook_update(
                "overtake-pre",
                "pre_tool_use",
                Some("new-tool"),
                false,
                vec![hook_run(
                    "new-tool-pre",
                    HookRunStatusDto::Success { elapsed_ms: 1 },
                    None,
                )],
                vec![],
            ),
        ),
        &mut app,
    );
    handle(
        tool_start(
            "hook-overtake",
            "new-tool",
            acp::ToolKind::Other,
            "new-tool",
            false,
        ),
        &mut app,
    );
    handle(
        tool_complete(
            "hook-overtake",
            "new-tool",
            acp::ToolKind::Other,
            "new-tool",
            None,
            None,
            false,
        ),
        &mut app,
    );

    let agent = &app.agents[&AgentId(0)];
    assert_eq!(agent.scrollback.len(), 2);
    assert!(agent.scrollback.entry(0).unwrap().hook_data.is_none());
    let new_data = agent
        .scrollback
        .entry(1)
        .unwrap()
        .hook_data
        .as_ref()
        .unwrap();
    assert_eq!(new_data.pre_hooks[0].name, "new-tool-pre");
}

#[test]
fn hidden_tool_and_turn_fence_preserve_unowned_live_hooks() {
    let mut hidden_app = make_app_with_agent("hook-hidden");
    handle(
        make_ext_session_notification(
            "hook-hidden",
            hook_update(
                "hidden-occurrence",
                "pre_tool_use",
                Some("hidden-tool"),
                false,
                vec![hook_run(
                    "hidden-hook",
                    HookRunStatusDto::Failed {
                        error: "hidden detail".into(),
                        elapsed_ms: 4,
                    },
                    Some("hidden output"),
                )],
                vec![],
            ),
        ),
        &mut hidden_app,
    );
    handle(
        tool_start(
            "hook-hidden",
            "hidden-tool",
            acp::ToolKind::Other,
            "task",
            false,
        ),
        &mut hidden_app,
    );
    let hidden_agent = &hidden_app.agents[&AgentId(0)];
    assert_eq!(lifecycle_count(hidden_agent), 1);
    let hidden_entry = hidden_agent.scrollback.entry(0).unwrap();
    assert!(hook_text(hidden_entry, DisplayMode::Expanded).contains("hidden detail"));

    let mut fenced_app = make_app_with_agent("hook-fence");
    handle(
        make_ext_session_notification(
            "hook-fence",
            hook_update(
                "fence-occurrence",
                "post_tool_use",
                Some("never-arrived"),
                false,
                vec![hook_run(
                    "fence-hook",
                    HookRunStatusDto::Success { elapsed_ms: 2 },
                    Some("fence output"),
                )],
                vec![],
            ),
        ),
        &mut fenced_app,
    );
    {
        let session = &mut fenced_app.agents.get_mut(&AgentId(0)).unwrap().session;
        session.current_prompt_id = Some("fenced-prompt".into());
        session.state = AgentState::TurnRunning;
    }
    handle(
        make_ext_session_notification(
            "hook-fence",
            GrowSessionUpdate::TurnCompleted {
                prompt_id: "fenced-prompt".into(),
                stop_reason: "end_turn".into(),
                agent_result: None,
                usage: None,
                identity: None,
            },
        ),
        &mut fenced_app,
    );
    let fenced_agent = &fenced_app.agents[&AgentId(0)];
    assert_eq!(lifecycle_count(fenced_agent), 1);
    let fenced_text = hook_text(
        fenced_agent.scrollback.entry(0).unwrap(),
        DisplayMode::Expanded,
    );
    assert!(fenced_text.contains("fence-hook"));
    assert!(fenced_text.contains("fence output"));
}

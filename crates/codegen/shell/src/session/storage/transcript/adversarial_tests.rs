use super::*;

use crate::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use acp_transport::protocol as acp;
use std::io::Write;
use std::path::{Path, PathBuf};

fn session_dir(home: &Path, id: &str) -> PathBuf {
    home.join("sessions")
        .join(crate::util::grow_home::encode_cwd_dirname("/project"))
        .join(id)
}

fn update_line(update: &SessionUpdate) -> Vec<u8> {
    let envelope = SessionUpdateEnvelope::from_update(update).unwrap();
    serde_json::to_vec(&envelope).unwrap()
}

fn write_updates(home: &Path, id: &str, updates: &[SessionUpdate]) {
    let mut contents = Vec::new();
    for update in updates {
        contents.extend(update_line(update));
        contents.push(b'\n');
    }
    std::fs::write(session_dir(home, id).join(UPDATES_FILE), contents).unwrap();
}

fn append_bytes(home: &Path, id: &str, bytes: &[u8]) {
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(session_dir(home, id).join(UPDATES_FILE))
        .unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
}

fn user_chunk(session_id: &str, text: &str, prompt_index: usize) -> SessionUpdate {
    let chunk = acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text))).meta(
        serde_json::json!({ "promptIndex": prompt_index })
            .as_object()
            .cloned(),
    );
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        acp::SessionId::new(session_id),
        acp::SessionUpdate::UserMessageChunk(chunk),
    )))
}

fn agent_chunk(session_id: &str, text: &str) -> SessionUpdate {
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        acp::SessionId::new(session_id),
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
            acp::TextContent::new(text),
        ))),
    )))
}

fn grow_update(session_id: &str, update: GrowUpdate) -> SessionUpdate {
    SessionUpdate::Grow(Box::new(GrowNotification {
        session_id: acp::SessionId::new(session_id),
        update,
        meta: None,
    }))
}

fn turn_completed(session_id: &str, prompt_id: &str, stop_reason: &str) -> SessionUpdate {
    grow_update(
        session_id,
        GrowUpdate::TurnCompleted {
            prompt_id: prompt_id.into(),
            identity: None,
            stop_reason: stop_reason.into(),
            agent_result: None,
            usage: None,
        },
    )
}

fn rewind_marker(session_id: &str, target_prompt_index: usize) -> SessionUpdate {
    grow_update(
        session_id,
        GrowUpdate::RewindMarker {
            target_prompt_index,
            created_at: "2026-09-29T00:00:00Z".into(),
        },
    )
}

fn goal_state(
    status: crate::session::goal_tracker::GoalStatus,
) -> crate::session::goal_tracker::GoalState {
    use crate::session::goal_tracker::{GoalPauseReason, GoalTracker};

    let mut tracker = GoalTracker::new();
    tracker
        .create_goal(
            "goal-1".into(),
            "ship safely".into(),
            None,
            "2026-09-29T00:00:00Z".into(),
        )
        .unwrap();
    match status {
        crate::session::goal_tracker::GoalStatus::Active => {}
        crate::session::goal_tracker::GoalStatus::Paused => {
            tracker.pause(GoalPauseReason::User);
        }
        crate::session::goal_tracker::GoalStatus::Blocked => {
            for index in 1..=3 {
                tracker
                    .report_blocked(
                        "waiting for user".into(),
                        index,
                        (index > 1).then_some(index - 1),
                    )
                    .unwrap();
            }
        }
        crate::session::goal_tracker::GoalStatus::BudgetLimited => {
            tracker.budget_limit();
        }
        crate::session::goal_tracker::GoalStatus::Complete => {
            tracker.complete();
        }
    }
    tracker.snapshot().unwrap().clone()
}

fn control_timeline(
    revision: u64,
    behavior: crate::session::behavior::BehaviorSnapshot,
    goal: Option<crate::session::goal_tracker::GoalState>,
) -> Timeline {
    let snapshot =
        crate::session::control::SessionControlSnapshot::new(revision, "grow", behavior, goal);
    let mut timeline = Timeline::default();
    timeline.record(snapshot.timeline_kind().unwrap()).unwrap();
    timeline
}

fn current_mode_update(session_id: &str, mode: &str, phase: Option<&str>) -> SessionUpdate {
    let update = acp::CurrentModeUpdate::new(acp::SessionModeId::new(mode)).meta(
        serde_json::json!({
            "grow/behavior": mode,
            "grow/planPhase": phase,
        })
        .as_object()
        .cloned(),
    );
    SessionUpdate::Acp(Box::new(acp::SessionNotification::new(
        acp::SessionId::new(session_id),
        acp::SessionUpdate::CurrentModeUpdate(update),
    )))
}

fn goal_updated(session_id: &str, goal: &crate::session::goal_tracker::GoalState) -> SessionUpdate {
    grow_update(
        session_id,
        crate::session::goal_notification::build_goal_updated(
            goal,
            goal.tokens_used,
            goal.elapsed_ms,
        ),
    )
}

fn update_control_event(home: &Path, id: &str, update: impl FnOnce(&mut serde_json::Value)) {
    let path = session_dir(home, id).join(TIMELINE_FILE);
    let contents = std::fs::read_to_string(&path).unwrap();
    let mut events = contents
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    let event = events
        .iter_mut()
        .find(|event| event.get("type").and_then(serde_json::Value::as_str) == Some("control"))
        .expect("fixture contains a Control event");
    update(event);
    let contents = events
        .iter()
        .map(serde_json::to_string)
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
        .join("\n")
        + "\n";
    std::fs::write(path, contents).unwrap();
}

fn displayed_messages(session: &TranscriptSession) -> Vec<String> {
    session
        .events
        .iter()
        .filter_map(|event| match &event.update {
            SessionUpdate::Acp(notification) => match &notification.update {
                acp::SessionUpdate::UserMessageChunk(chunk) => match &chunk.content {
                    acp::ContentBlock::Text(text) => Some(format!("user:{}", text.text)),
                    _ => None,
                },
                acp::SessionUpdate::AgentMessageChunk(chunk) => match &chunk.content {
                    acp::ContentBlock::Text(text) => Some(format!("assistant:{}", text.text)),
                    _ => None,
                },
                _ => None,
            },
            SessionUpdate::Grow(notification) => match &notification.update {
                GrowUpdate::TurnCompleted { stop_reason, .. } => {
                    Some(format!("terminal:{stop_reason}"))
                }
                _ => None,
            },
            SessionUpdate::ResponseReplayProjection(_) => None,
        })
        .collect()
}

#[test]
fn captured_updates_ignore_completed_torn_tail_and_later_rows() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "root", "/project", &Timeline::default(), false);

    let first = user_chunk("root", "A", 0);
    let partial = update_line(&user_chunk("root", "not visible until completed", 1));
    let mut initial = update_line(&first);
    initial.push(b'\n');
    initial.extend_from_slice(&partial[..partial.len() / 2]);
    std::fs::write(session_dir(home.path(), "root").join(UPDATES_FILE), initial).unwrap();

    let mut snapshot = capture_tree_at("root", home.path()).unwrap();

    let mut append = partial[partial.len() / 2..].to_vec();
    append.push(b'\n');
    append.extend(update_line(&user_chunk("root", "B", 2)));
    append.push(b'\n');
    append.extend_from_slice(b"{bad json}\n");
    append_bytes(home.path(), "root", &append);

    let session = snapshot.read_session("root").unwrap();
    assert_eq!(displayed_messages(&session), ["user:A"]);
}

#[test]
fn captured_updates_fail_when_the_source_is_truncated() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "root", "/project", &Timeline::default(), false);
    write_updates(
        home.path(),
        "root",
        &[user_chunk("root", "A", 0), user_chunk("root", "B", 1)],
    );
    let path = session_dir(home.path(), "root").join(UPDATES_FILE);
    let captured_len = std::fs::metadata(&path).unwrap().len();
    let mut snapshot = capture_tree_at("root", home.path()).unwrap();

    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(captured_len / 2)
        .unwrap();

    let error = snapshot.read_session("root").unwrap_err();
    assert!(error.to_string().contains("truncated"), "{error}");
}

#[test]
fn foreign_session_update_fails_even_when_rewind_would_drop_it() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "root", "/project", &Timeline::default(), false);
    write_updates(
        home.path(),
        "root",
        &[
            user_chunk("root", "first", 0),
            user_chunk("root", "dead branch", 1),
            agent_chunk("other-session", "foreign complete update"),
            rewind_marker("root", 1),
        ],
    );
    let mut snapshot = capture_tree_at("root", home.path()).unwrap();

    let error = snapshot.read_session("root").unwrap_err();
    assert!(
        error
            .to_string()
            .contains("different or missing session identity")
    );
}

#[test]
fn rewind_after_cancel_preserves_the_current_continuation_branch() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "root", "/project", &Timeline::default(), false);
    write_updates(
        home.path(),
        "root",
        &[
            user_chunk("root", "first prompt", 0),
            agent_chunk("root", "first response"),
            turn_completed("root", "prompt-0", "cancelled"),
            user_chunk("root", "rewound prompt", 1),
            agent_chunk("root", "discarded response"),
            rewind_marker("root", 1),
            user_chunk("root", "continued prompt", 1),
            agent_chunk("root", "continued response"),
        ],
    );
    let mut snapshot = capture_tree_at("root", home.path()).unwrap();

    let session = snapshot.read_session("root").unwrap();
    assert_eq!(
        displayed_messages(&session),
        [
            "user:first prompt",
            "assistant:first response",
            "terminal:cancelled",
            "user:continued prompt",
            "assistant:continued response",
        ]
    );
}

#[test]
fn captured_root_and_child_topology_does_not_gain_later_root_spawn() {
    let home = tempfile::tempdir().unwrap();
    let mut root_timeline = Timeline::default();
    let first_spawn = spawn("child", "root");
    root_timeline
        .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(
            first_spawn.clone(),
        )))
        .unwrap();
    let first_spawn_seq = root_timeline.events().last().unwrap().seq.get();

    let mut child_timeline = Timeline::default();
    child_timeline
        .record(TimelineEventKind::SubagentSeed(child_seed(
            "root",
            first_spawn_seq,
            &first_spawn,
        )))
        .unwrap();
    write_session(home.path(), "root", "/project", &root_timeline, false);
    write_session(home.path(), "child", "/project", &child_timeline, true);
    write_updates(home.path(), "root", &[]);
    write_updates(home.path(), "child", &[]);
    write_updates(
        home.path(),
        "root",
        &[user_chunk("root", "captured root body", 0)],
    );

    let mut root_snapshot = capture_tree_at("root", home.path()).unwrap();
    let child_snapshot = capture_tree_at("child", home.path()).unwrap();
    assert_eq!(
        root_snapshot
            .nodes
            .iter()
            .map(|node| node.session_id.as_str())
            .collect::<Vec<_>>(),
        ["root", "child"]
    );
    assert_eq!(
        child_snapshot
            .nodes
            .iter()
            .map(|node| node.session_id.as_str())
            .collect::<Vec<_>>(),
        ["child"]
    );

    let mut expanded_timeline = Timeline::from_events(root_timeline.events().to_vec()).unwrap();
    expanded_timeline
        .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn(
            "later-child",
            "root",
        ))))
        .unwrap();
    let late_event = expanded_timeline.events().last().unwrap();
    let mut late_line = serde_json::to_vec(late_event).unwrap();
    late_line.push(b'\n');
    let timeline_path = session_dir(home.path(), "root").join(TIMELINE_FILE);
    let mut timeline_file = std::fs::OpenOptions::new()
        .append(true)
        .open(timeline_path)
        .unwrap();
    timeline_file.write_all(&late_line).unwrap();
    timeline_file.flush().unwrap();

    assert_eq!(root_snapshot.nodes.len(), 2);
    assert_eq!(root_snapshot.nodes[0].children, ["child"]);
    assert_eq!(child_snapshot.nodes.len(), 1);
    assert_eq!(child_snapshot.nodes[0].session_id, "child");

    let root_session = root_snapshot.read_session("root").unwrap();
    let restored_spawns = root_session
        .events
        .iter()
        .filter_map(|event| match &event.update {
            SessionUpdate::Grow(notification) => match &notification.update {
                GrowUpdate::SubagentSpawned {
                    child_session_id, ..
                } => Some(child_session_id.as_str()),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(restored_spawns, ["child"]);
}

#[test]
fn captured_timeline_fails_when_the_source_is_truncated() {
    let home = tempfile::tempdir().unwrap();
    let mut timeline = Timeline::default();
    timeline
        .record(TimelineEventKind::Subagent(SubagentEvent::Spawned(spawn(
            "child", "root",
        ))))
        .unwrap();
    write_session(home.path(), "root", "/project", &timeline, false);

    let storage = JsonlStorageAdapter::with_root(home.path().to_path_buf());
    let opened = storage
        .open_session_by_id_shared_read("root")
        .unwrap()
        .unwrap();
    let mut budget = Budget::default();
    let captured = CapturedLedger::open(opened.directory(), TIMELINE_FILE, &mut budget).unwrap();
    let path = session_dir(home.path(), "root").join(TIMELINE_FILE);
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(captured.len / 2)
        .unwrap();

    let error = captured
        .versioned::<chat_state::TimelineEvent>(
            u64::from(chat_state::TIMELINE_SCHEMA_VERSION),
            &mut budget,
        )
        .unwrap_err();
    assert!(error.to_string().contains("truncated"), "{error}");
}

#[test]
fn capture_rejects_sparse_oversized_updates_before_reading() {
    let home = tempfile::tempdir().unwrap();
    write_session(home.path(), "root", "/project", &Timeline::default(), false);
    std::fs::File::create(session_dir(home.path(), "root").join(UPDATES_FILE))
        .unwrap()
        .set_len(MAX_SOURCE_BYTES + 1)
        .unwrap();

    let error = capture_tree_at("root", home.path())
        .err()
        .expect("oversized source must fail");
    assert!(error.to_string().contains("read budget"), "{error}");
}

#[test]
fn captured_control_snapshot_repairs_stale_goal_projection_without_rewriting_source() {
    use crate::session::behavior::BehaviorSnapshot;
    use crate::session::goal_tracker::GoalStatus;

    let home = tempfile::tempdir().unwrap();
    let active = goal_state(GoalStatus::Active);
    let paused = goal_state(GoalStatus::Paused);
    let cases = [("paused", Some(paused)), ("cleared", None)];

    for (id, final_goal) in cases {
        let timeline = control_timeline(7, BehaviorSnapshot::normal(), final_goal.clone());
        write_session(home.path(), id, "/project", &timeline, false);
        write_updates(
            home.path(),
            id,
            &[
                user_chunk(id, "question", 0),
                agent_chunk(id, "answer"),
                current_mode_update(id, "goal", None),
                goal_updated(id, &active),
            ],
        );
        let updates_path = session_dir(home.path(), id).join(UPDATES_FILE);
        let source_before = std::fs::read(&updates_path).unwrap();

        let session = read_session_at(id, home.path()).unwrap();
        assert_eq!(
            displayed_messages(&session),
            ["user:question", "assistant:answer"]
        );
        let tail = &session.events[session.events.len() - 3..];
        let SessionUpdate::Acp(notification) = &tail[0].update else {
            panic!("expected synthesized CurrentModeUpdate");
        };
        let acp::SessionUpdate::CurrentModeUpdate(mode) = &notification.update else {
            panic!("expected synthesized CurrentModeUpdate");
        };
        assert_eq!(mode.current_mode_id.0.as_ref(), "normal");
        let expected_goal = final_goal.as_ref().map_or_else(
            crate::session::goal_notification::build_goal_cleared,
            |goal| {
                crate::session::goal_notification::build_goal_updated(
                    goal,
                    goal.tokens_used,
                    goal.elapsed_ms,
                )
            },
        );
        let SessionUpdate::Grow(notification) = &tail[1].update else {
            panic!("expected synthesized GoalUpdated");
        };
        assert_eq!(notification.update, expected_goal);
        for event in tail {
            assert_eq!(event.timestamp_ms, None);
            assert!(event.simulated_source);
        }
        assert_eq!(std::fs::read(updates_path).unwrap(), source_before);
    }
}

#[test]
fn captured_matching_control_snapshot_does_not_duplicate_mode_or_goal() {
    use crate::session::behavior::BehaviorSnapshot;
    use crate::session::goal_tracker::GoalStatus;

    let home = tempfile::tempdir().unwrap();
    let goal = goal_state(GoalStatus::Active);
    let timeline = control_timeline(
        3,
        BehaviorSnapshot::selected(tool_types::BehaviorId::Goal),
        Some(goal.clone()),
    );
    write_session(home.path(), "root", "/project", &timeline, false);
    write_updates(
        home.path(),
        "root",
        &[
            current_mode_update("root", "goal", None),
            goal_updated("root", &goal),
        ],
    );

    let session = read_session_at("root", home.path()).unwrap();
    assert_eq!(session.events.len(), 2);
    assert!(session.events.iter().all(|event| !event.simulated_source));
}

#[test]
fn captured_plan_control_snapshot_projects_each_recorded_phase_without_approval() {
    use crate::session::behavior::{BehaviorSnapshot, BehaviorState, PlanPhase};

    let home = tempfile::tempdir().unwrap();
    let cases = [
        (PlanPhase::Drafting, "drafting"),
        (PlanPhase::AwaitingApproval, "awaiting_approval"),
        (PlanPhase::Executing, "executing"),
        (PlanPhase::Amending, "amending"),
    ];
    for (index, (phase, expected)) in cases.into_iter().enumerate() {
        let id = format!("plan-{index}");
        let mut behavior = BehaviorSnapshot::selected(tool_types::BehaviorId::Plan);
        behavior.state = BehaviorState::Plan(phase);
        let timeline = control_timeline(index as u64 + 1, behavior, None);
        write_session(home.path(), &id, "/project", &timeline, false);
        write_updates(home.path(), &id, &[user_chunk(&id, "plan", 0)]);

        let session = read_session_at(&id, home.path()).unwrap();
        let mode = session
            .events
            .iter()
            .rev()
            .find_map(|event| match &event.update {
                SessionUpdate::Acp(notification) => match &notification.update {
                    acp::SessionUpdate::CurrentModeUpdate(mode) => Some(mode),
                    _ => None,
                },
                _ => None,
            })
            .unwrap();
        assert_eq!(mode.current_mode_id.0.as_ref(), "plan");
        assert_eq!(
            mode.meta
                .as_ref()
                .and_then(|meta| meta.get("grow/planPhase"))
                .and_then(serde_json::Value::as_str),
            Some(expected),
        );
        assert!(session.events.last().unwrap().simulated_source);
    }
}

#[test]
fn captured_control_timeline_rejects_invalid_snapshots() {
    use crate::session::behavior::BehaviorSnapshot;
    use crate::session::goal_tracker::GoalStatus;

    let home = tempfile::tempdir().unwrap();
    let cases = ["architecture", "revision", "ownership"];
    for (index, case) in cases.into_iter().enumerate() {
        let id = format!("invalid-{case}");
        let timeline = control_timeline(index as u64 + 1, BehaviorSnapshot::normal(), None);
        write_session(home.path(), &id, "/project", &timeline, false);
        write_updates(home.path(), &id, &[user_chunk(&id, "hello", 0)]);
        update_control_event(home.path(), &id, |event| match case {
            "architecture" => {
                event["event"]["snapshot"]["architecture_version"] =
                    (crate::session::control::SESSION_CONTROL_ARCHITECTURE_VERSION + 1).into();
            }
            "revision" => {
                event["event"]["revision"] = (index as u64 + 20).into();
            }
            "ownership" => {
                event["event"]["snapshot"]["goal"] =
                    serde_json::to_value(goal_state(GoalStatus::Active)).unwrap();
            }
            _ => unreachable!(),
        });

        let error = read_session_at(&id, home.path()).unwrap_err();
        let message = error.to_string();
        match case {
            "architecture" => assert!(
                message.contains("Session Control architecture"),
                "{message}"
            ),
            "revision" => assert!(message.contains("control event revision"), "{message}"),
            "ownership" => assert!(
                message.contains("active Goal and Goal Behavior ownership do not agree"),
                "{message}"
            ),
            _ => unreachable!(),
        }
    }
}

fn admitted_timeline(
    request_id: &str,
    attempt: u32,
    items: Vec<sampling_types::ConversationItem>,
    quarantined_tool_exchanges: usize,
) -> chat_state::Timeline {
    let mut timeline = chat_state::Timeline::default();
    let turn = chat_state::TurnId(1);
    let step = chat_state::StepId { turn, index: 0 };
    timeline
        .record(chat_state::TimelineEventKind::Turn(
            chat_state::TurnEvent::Started {
                id: turn,
                input_ids: Vec::new(),
                identity: chat_state::TurnIdentity {
                    origin: "user".into(),
                    turn_kind: "internal".into(),
                    goal_id: None,
                    goal_definition_revision: None,
                    stage_id: None,
                },
                model_id: "model".into(),
                input_message_count: 0,
                prompt_index: 0,
                prompt_text: "test".into(),
                input_kind: chat_state::TurnInputKind::Prompt,
                redirect_kind: None,
            },
        ))
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Step(
            chat_state::StepEvent::Started { id: step },
        ))
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Request(
            chat_state::RequestEvent::Started {
                id: request_id.into(),
                turn,
                step,
                model_id: "model".into(),
                input_message_count: 0,
                tool_count: 0,
            },
        ))
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Request(
            chat_state::RequestEvent::Completed {
                id: request_id.into(),
                duration_ms: 1,
                time_to_first_token_ms: None,
                usage: chat_state::RequestUsage::default(),
                response_message_count: items.len(),
                attempt,
                provider_terminal: None,
            },
        ))
        .unwrap();
    timeline
        .record(chat_state::TimelineEventKind::Messages(
            chat_state::MessageEvent {
                cause: chat_state::MessageCause::Assistant,
                items,
                surface: chat_state::SurfaceOp::Append,
                response_admission: Some(chat_state::ResponseAdmission {
                    identity: chat_state::ResponseAdmissionIdentity {
                        request_id: request_id.into(),
                        attempt,
                    },
                    quarantined_tool_exchanges,
                }),
            },
        ))
        .unwrap();
    timeline
}

#[test]
fn canonical_response_retains_anchor_time_and_discards_other_attempt() {
    let home = tempfile::tempdir().unwrap();
    let timeline = admitted_timeline(
        "accepted",
        1,
        vec![sampling_types::ConversationItem::assistant(
            "canonical body",
        )],
        0,
    );
    write_session(home.path(), "root", "/project", &timeline, false);
    let candidate = |request: &str, body: &str| {
        let mut update = agent_chunk("root", body);
        if let SessionUpdate::Acp(notification) = &mut update {
            notification.meta = serde_json::json!({
                "samplingRequestId": request, "samplingAttempt": 1,
                "agentTimestampMs": 1000, "streamStartMs": 900,
                "turnStartMs": 800, "promptId": "prompt-1",
            })
            .as_object()
            .cloned();
        }
        update
    };
    write_updates(
        home.path(),
        "root",
        &[
            candidate("rejected", "never show this candidate"),
            candidate("accepted", ""),
            turn_completed("root", "prompt-1", "cancelled"),
        ],
    );
    let session = read_session_at("root", home.path()).unwrap();
    let mut bodies = Vec::new();
    for event in session.events {
        if let SessionUpdate::Acp(notification) = event.update {
            let meta = notification.meta.unwrap();
            assert_eq!(meta["agentTimestampMs"], 1000);
            assert_eq!(meta["promptId"], "prompt-1");
            if let acp::SessionUpdate::AgentMessageChunk(chunk) = notification.update {
                let acp::ContentBlock::Text(text) = chunk.content else {
                    panic!("text");
                };
                bodies.push(text.text);
            }
        }
    }
    assert_eq!(bodies, ["canonical body"]);
}

#[test]
fn empty_committed_display_does_not_hide_timeline_facts() {
    let home = tempfile::tempdir().unwrap();
    let timeline = admitted_timeline(
        "accepted",
        1,
        vec![sampling_types::ConversationItem::assistant("body")],
        0,
    );
    write_session(home.path(), "root", "/project", &timeline, false);
    write_updates(home.path(), "root", &[]);
    assert!(
        read_session_at("root", home.path())
            .unwrap_err()
            .to_string()
            .contains("no committed display")
    );
}

#[test]
fn completed_hook_is_reconstructed_without_running_any_handler() {
    use chat_state::*;
    let home = tempfile::tempdir().unwrap();
    let mut timeline = Timeline::default();
    for event in [
        HookEvent::Triggered {
            occurrence_id: "history-hook".into(),
            event: HookEventType::SessionStart,
            gate: HookGateKind::Observe,
            cause: HookCause::Session {
                session_id: "root".into(),
            },
            config_generation: 1,
            handlers: vec![HookHandlerPlan {
                index: 0,
                run_id: "run-1".into(),
                name: "historical-handler".into(),
                provenance: HookHandlerProvenance::ProjectFile,
                kind: HookHandlerKind::Command,
                failure_policy: HookFailurePolicy::Allow,
                action: HookHandlerPlanAction::Execute,
            }],
        },
        HookEvent::RunStarted {
            occurrence_id: "history-hook".into(),
            run_id: "run-1".into(),
            handler_index: 0,
        },
        HookEvent::RunFinished {
            occurrence_id: "history-hook".into(),
            run_id: "run-1".into(),
            handler_index: 0,
            elapsed_ms: 41,
            outcome: HookRunOutcome::Cancelled,
            control: HookRunControl::None,
        },
        HookEvent::Completed {
            occurrence_id: "history-hook".into(),
            decision: HookAggregateDecision::Observe,
        },
    ] {
        timeline.record(TimelineEventKind::Hook(event)).unwrap();
    }
    write_session(home.path(), "root", "/project", &timeline, false);
    write_updates(home.path(), "root", &[user_chunk("root", "hello", 0)]);
    let before = std::fs::read(session_dir(home.path(), "root").join(TIMELINE_FILE)).unwrap();
    let session = read_session_at("root", home.path()).unwrap();
    let hooks = session
        .events
        .iter()
        .filter_map(|event| match &event.update {
            SessionUpdate::Grow(notification) => match &notification.update {
                GrowUpdate::HookExecution {
                    occurrence_id,
                    is_snapshot,
                    runs,
                    ..
                } => Some((occurrence_id, is_snapshot, runs)),
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].0, "history-hook");
    assert!(*hooks[0].1);
    assert_eq!(hooks[0].2[0].name, "historical-handler");
    assert!(
        matches!(&hooks[0].2[0].status, crate::extensions::notification::HookRunStatusDto::Failed { error, elapsed_ms: 41 } if error == "hook cancelled")
    );
    assert_eq!(
        before,
        std::fs::read(session_dir(home.path(), "root").join(TIMELINE_FILE)).unwrap()
    );
}

#[test]
fn interrupting_parent_receipt_is_restored_from_timeline_without_delivery() {
    use chat_state::*;
    let home = tempfile::tempdir().unwrap();
    let body = "Stop the old approach and inspect the failing test.";
    let hash = blake3::hash(body.as_bytes()).to_hex().to_string();
    let source = NotificationSource::ParentMessage {
        parent_session_id: "parent".into(),
        message_id: "message-1".into(),
        interrupt: true,
    };
    let source_version = NotificationSourceVersion::Ordinal {
        value: PARENT_MESSAGE_SOURCE_VERSION,
    };
    let id = notification_id("root", &source, &source_version).unwrap();
    let mut timeline = Timeline::default();
    timeline
        .record(TimelineEventKind::Notification(
            NotificationEvent::Received {
                id: id.clone(),
                owner_session_id: "root".into(),
                source,
                source_version,
                payload_ref: NotificationPayloadRef {
                    blake3: hash.clone(),
                    bytes: body.len() as u64,
                },
            },
        ))
        .unwrap();
    write_session(home.path(), "root", "/project", &timeline, false);
    write_updates(
        home.path(),
        "root",
        &[user_chunk("root", "initial request", 0)],
    );
    let artifacts = session_dir(home.path(), "root").join("artifacts/notifications");
    std::fs::create_dir_all(&artifacts).unwrap();
    let payload = artifacts.join(format!("{hash}.txt"));
    std::fs::write(&payload, body).unwrap();
    let before = std::fs::read(session_dir(home.path(), "root").join(TIMELINE_FILE)).unwrap();

    for available in [true, false] {
        if !available {
            std::fs::remove_file(&payload).unwrap();
        }
        let session = read_session_at("root", home.path()).unwrap();
        let receipts = session
            .events
            .iter()
            .filter_map(|event| match &event.update {
                SessionUpdate::Grow(notification) => match &notification.update {
                    GrowUpdate::UiNotice(notice) if notice.correlation_id == id => Some(notice),
                    _ => None,
                },
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(receipts.len(), 1);
        let details: crate::extensions::notification::AgentMessageNotice =
            serde_json::from_str(receipts[0].details.as_ref().unwrap()).unwrap();
        assert!(details.interrupt);
        assert_eq!(details.message.as_deref(), available.then_some(body));
        if !available {
            assert_eq!(receipts[0].message, "Message unavailable");
        }
        assert_eq!(
            before,
            std::fs::read(session_dir(home.path(), "root").join(TIMELINE_FILE)).unwrap()
        );
    }
}

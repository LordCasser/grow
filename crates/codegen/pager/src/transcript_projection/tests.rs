use super::*;

use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use acp_transport::protocol as acp;
use shell::extensions::notification::{
    SessionNotification as GrowNotification, SessionUpdate as GrowUpdate,
};
use tools::types::TaskSnapshot;

use crate::scrollback::blocks::{BgTaskKind, ExecuteToolCallBlock, ToolCallBlock};
use crate::scrollback::entry::EntryId;

fn event(update: SessionUpdate) -> TranscriptEvent {
    TranscriptEvent {
        update,
        timestamp_ms: None,
        simulated_source: false,
    }
}

fn acp_event(update: acp::SessionUpdate, prompt_id: Option<&str>) -> TranscriptEvent {
    let mut meta = acp::Meta::new();
    if let Some(prompt_id) = prompt_id {
        meta.insert(
            "promptId".into(),
            serde_json::Value::String(prompt_id.into()),
        );
    }
    event(SessionUpdate::Acp(Box::new(
        acp::SessionNotification::new(acp::SessionId::new("projection-test"), update)
            .meta((!meta.is_empty()).then_some(meta)),
    )))
}

fn grow_event(update: GrowUpdate) -> TranscriptEvent {
    event(SessionUpdate::Grow(Box::new(GrowNotification {
        session_id: acp::SessionId::new("projection-test"),
        update,
        meta: None,
    })))
}

fn tool_call(id: &str) -> acp::SessionUpdate {
    acp::SessionUpdate::ToolCall(
        acp::ToolCall::new(
            acp::ToolCallId::new(std::sync::Arc::<str>::from(id)),
            "sleep 60",
        )
        .kind(acp::ToolKind::Execute)
        .status(acp::ToolCallStatus::Pending)
        .raw_input(Some(serde_json::json!({
            "command": "sleep 60",
            "description": "wait for a process",
        }))),
    )
}

fn tool_terminal(id: &str, status: acp::ToolCallStatus) -> acp::SessionUpdate {
    acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(
        acp::ToolCallId::new(std::sync::Arc::<str>::from(id)),
        acp::ToolCallUpdateFields::new().status(Some(status)),
    ))
}

fn user_message(text: &str, hide_from_scrollback: bool) -> acp::SessionUpdate {
    let mut chunk_meta = acp::Meta::new();
    if hide_from_scrollback {
        chunk_meta.insert(
            crate::acp::meta::user_message_chunk_meta::HIDE_FROM_SCROLLBACK.into(),
            serde_json::Value::Bool(true),
        );
    }
    acp::SessionUpdate::UserMessageChunk(
        acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(text)))
            .meta((!chunk_meta.is_empty()).then_some(chunk_meta)),
    )
}

fn assistant_message(text: &str) -> acp::SessionUpdate {
    acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(acp::ContentBlock::Text(
        acp::TextContent::new(text),
    )))
}

fn turn_completed(prompt_id: &str, stop_reason: &str) -> TranscriptEvent {
    grow_event(GrowUpdate::TurnCompleted {
        prompt_id: prompt_id.into(),
        identity: None,
        stop_reason: stop_reason.into(),
        agent_result: None,
        usage: None,
    })
}

fn execute_entries(
    projection: &TranscriptProjection,
) -> Vec<(
    EntryId,
    &crate::scrollback::entry::ScrollbackEntry,
    &ExecuteToolCallBlock,
)> {
    projection
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| match &entry.block {
            RenderBlock::ToolCall(ToolCallBlock::Execute(block)) => Some((id, entry, block)),
            _ => None,
        })
        .collect()
}

fn assistant_entries(projection: &TranscriptProjection) -> Vec<(EntryId, String)> {
    projection
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| match &entry.block {
            RenderBlock::AgentMessage(block) => Some((id, block.text())),
            _ => None,
        })
        .collect()
}

fn markdown(projection: &TranscriptProjection) -> String {
    crate::scrollback::export::render_entries_to_full_markdown(
        projection.scrollback.iter_entries().map(|(_, entry)| entry),
        &[],
    )
}

#[test]
fn cancelled_tool_stays_pending_across_next_prompt_and_assistant_response() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(tool_call("tool-p1"), Some("p1")));
    let (tool_id, _, _) = execute_entries(&projection)
        .into_iter()
        .next()
        .expect("pending Execute should create one row");

    projection.apply(turn_completed("p1", "cancelled"));
    projection.apply(acp_event(user_message("next prompt", false), Some("p2")));
    projection.apply(acp_event(assistant_message("response p2"), Some("p2")));

    let tools = execute_entries(&projection);
    assert_eq!(tools.len(), 1, "the pending tool row must remain unique");
    assert_eq!(tools[0].0, tool_id);
    assert!(tools[0].1.is_running, "the snapshot has no tool terminal");
    assert_eq!(
        tools[0].2.started_at, None,
        "replay must not invent a local start time"
    );
    assert_eq!(
        tools[0].2.elapsed_ms, None,
        "replay must not invent elapsed time"
    );

    let rendered = markdown(&projection);
    assert!(rendered.contains("still running at the captured snapshot"));
    assert!(rendered.contains("Turn p1 · cancelled"));
    assert_eq!(assistant_entries(&projection).len(), 1);
    assert_eq!(assistant_entries(&projection)[0].1, "response p2");
}

#[test]
fn task_elapsed_uses_source_time_and_terminal_progress_cannot_resurrect_it() {
    let mut projection = TranscriptProjection::default();
    let timed = |value: serde_json::Value, at| {
        let mut event = grow_event(serde_json::from_value(value).unwrap());
        event.timestamp_ms = Some(at);
        event
    };
    projection.apply(timed(serde_json::json!({"sessionUpdate":"subagent_spawned", "subagent_id":"s", "parent_session_id":"projection-test", "child_session_id":"s", "description":"inspect", "subagent_type":"explore"}), 1_000));
    let frame = projection.surfaces.frame(Some(61_000), false);
    assert_eq!(
        projection.surfaces.subagents["s"].display_elapsed_at(frame.now()),
        Duration::from_secs(60)
    );
    assert_eq!(
        projection.surfaces.frame(Some(61_000), false),
        frame,
        "paused source clock freezes every task surface"
    );
    projection.apply(timed(serde_json::json!({"sessionUpdate":"subagent_finished", "subagent_id":"s", "child_session_id":"s", "status":"completed", "tool_calls":2, "turns":1, "duration_ms":70_000, "tokens_used":20}), 71_000));
    projection.apply(timed(serde_json::json!({"sessionUpdate":"subagent_progress", "subagent_id":"s", "parent_session_id":"projection-test", "child_session_id":"s", "duration_ms":80_000, "turn_count":2, "tool_call_count":3, "tokens_used":30, "context_window_tokens":100, "context_usage_pct":30, "tools_used":[], "error_count":0}), 81_000));
    assert!(projection.surfaces.subagents["s"].finished);
    assert_eq!(
        projection.surfaces.subagents["s"]
            .display_elapsed_at(projection.surfaces.frame(Some(1_000_000), true).now()),
        Duration::from_secs(70)
    );
}

#[test]
fn interrupted_turn_followup_owns_the_status_row_and_delayed_terminal_does_not_hide_it() {
    let mut projection = TranscriptProjection::default();
    let mut first = acp_event(user_message("first", false), Some("p1"));
    first.timestamp_ms = Some(1_000);
    projection.apply(first);
    projection.apply(turn_completed("p1", "cancelled"));
    assert!(!projection.surfaces.turn_active);
    let mut second = acp_event(user_message("followup", false), Some("p2"));
    second.timestamp_ms = Some(10_000);
    projection.apply(second);
    projection.apply(turn_completed("p1", "cancelled"));
    assert!(projection.surfaces.turn_active);
    assert_eq!(projection.surfaces.turn_started_ms, Some(10_000));
    projection.apply(turn_completed("p2", "end_turn"));
    assert!(!projection.surfaces.turn_active);
}

#[test]
fn shared_goal_workflow_and_schedule_surfaces_cannot_resurrect_cleared_history() {
    let mut p = TranscriptProjection::default();
    let goal = |status: &str| {
        grow_event(serde_json::from_value(serde_json::json!({"sessionUpdate":"goal_updated", "goal_id":"g", "objective":"inspect", "status":status, "token_budget":null, "elapsed_ms":1_000, "created_at":"saved", "updated_at":"saved"})).unwrap())
    };
    p.apply(goal("active"));
    assert!(
        p.surfaces
            .goal
            .as_ref()
            .is_some_and(|g| g.status == crate::app::session::GoalDisplayStatus::Active)
    );
    p.apply(goal("paused"));
    assert!(
        p.surfaces
            .goal
            .as_ref()
            .is_some_and(|g| g.status == crate::app::session::GoalDisplayStatus::Paused)
    );
    p.apply(goal("cleared"));
    p.apply(goal("active"));
    assert!(p.surfaces.goal.is_none());
    let workflow = |revision, status: &str| {
        grow_event(serde_json::from_value(serde_json::json!({"sessionUpdate":"workflow_updated", "run_id":"r", "name":"saved run", "objective":"inspect", "revision":revision, "status":status, "elapsed_ms":1_000})).unwrap())
    };
    p.apply(workflow(2, "active"));
    assert_eq!(p.surfaces.workflows.len(), 1);
    assert!(!p.surfaces.workflows[0].management_available);
    p.apply(workflow(3, "cleared"));
    p.apply(workflow(1, "active"));
    assert!(p.surfaces.workflows.is_empty());
    p.apply(grow_event(GrowUpdate::ScheduledTaskCreated {
        task_id: "loop".into(),
        prompt: "saved".into(),
        human_schedule: "hourly".into(),
        next_fire_at: None,
    }));
    assert_eq!(p.surfaces.scheduled.len(), 1);
    p.apply(grow_event(GrowUpdate::ScheduledTaskDeleted {
        task_id: "loop".into(),
    }));
    assert!(p.surfaces.scheduled.is_empty());
}

#[test]
fn cross_turn_tool_terminal_updates_the_original_execute_row_once() {
    for status in [acp::ToolCallStatus::Failed, acp::ToolCallStatus::Completed] {
        let mut projection = TranscriptProjection::default();
        projection.apply(acp_event(tool_call("tool-p1"), Some("p1")));
        let original_id = execute_entries(&projection)[0].0;
        projection.apply(turn_completed("p1", "cancelled"));
        projection.apply(acp_event(user_message("next prompt", false), Some("p2")));
        projection.apply(acp_event(assistant_message("response p2"), Some("p2")));

        projection.apply(acp_event(tool_terminal("tool-p1", status), Some("p2")));

        let tools = execute_entries(&projection);
        assert_eq!(
            tools.len(),
            1,
            "terminal status {status:?} must not duplicate the tool"
        );
        assert_eq!(
            tools[0].0, original_id,
            "terminal status {status:?} must replace in place"
        );
        assert!(!tools[0].1.is_running);
        assert_eq!(tools[0].2.started_at, None);
        assert_eq!(tools[0].2.elapsed_ms, None);
        match status {
            acp::ToolCallStatus::Failed => {
                assert_eq!(tools[0].2.error.as_deref(), Some("Command failed"));
            }
            acp::ToolCallStatus::Completed => assert_eq!(tools[0].2.error, None),
            _ => unreachable!(),
        }
        assert_eq!(assistant_entries(&projection).len(), 1);
    }
}

#[test]
fn late_old_terminal_does_not_split_new_assistant_message_and_deduplicates() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(assistant_message("answer p1"), Some("p1")));
    projection.apply(acp_event(user_message("prompt p2", false), Some("p2")));
    projection.apply(acp_event(assistant_message("A"), Some("p2")));
    let p2_entry_id = assistant_entries(&projection)[1].0;

    projection.apply(turn_completed("p1", "cancelled"));
    let notices_after_first_terminal = projection
        .scrollback
        .iter_entries()
        .filter(|(_, entry)| matches!(&entry.block, RenderBlock::Notice(_)))
        .count();
    projection.apply(turn_completed("p1", "cancelled"));
    let notices_after_duplicate = projection
        .scrollback
        .iter_entries()
        .filter(|(_, entry)| matches!(&entry.block, RenderBlock::Notice(_)))
        .count();
    projection.apply(acp_event(assistant_message("B"), Some("p2")));

    let assistant = assistant_entries(&projection);
    assert_eq!(
        assistant.len(),
        2,
        "p2 chunks must stay in one assistant row"
    );
    assert_eq!(assistant[0].1, "answer p1");
    assert_eq!(assistant[1], (p2_entry_id, "AB".to_owned()));
    assert_eq!(notices_after_first_terminal, 1);
    assert_eq!(notices_after_duplicate, notices_after_first_terminal);
    assert!(
        projection
            .scrollback
            .get_by_id(p2_entry_id)
            .unwrap()
            .is_running,
        "the late p1 terminal must leave the p2 response open",
    );
}

#[test]
fn duplicate_user_echo_does_not_split_the_existing_reply() {
    for hidden in [false, true] {
        let mut projection = TranscriptProjection::default();
        let echo = || {
            let mut chunk_meta = acp::Meta::new();
            chunk_meta.insert("messageId".into(), serde_json::json!("p1"));
            chunk_meta.insert(
                crate::acp::meta::user_message_chunk_meta::HIDE_FROM_SCROLLBACK.into(),
                serde_json::json!(hidden),
            );
            acp_event(
                acp::SessionUpdate::UserMessageChunk(
                    acp::ContentChunk::new(acp::ContentBlock::Text(acp::TextContent::new(
                        "prompt",
                    )))
                    .meta(Some(chunk_meta)),
                ),
                Some("p1"),
            )
        };
        projection.apply(echo());
        projection.apply(acp_event(assistant_message("A"), Some("p1")));
        let reply_id = assistant_entries(&projection)[0].0;
        projection.apply(echo());
        projection.apply(acp_event(assistant_message("B"), Some("p1")));
        assert_eq!(
            assistant_entries(&projection),
            vec![(reply_id, "AB".into())]
        );
        assert_eq!(
            projection
                .scrollback
                .iter_entries()
                .filter(|(_, entry)| matches!(entry.block, RenderBlock::UserPrompt(_)))
                .count(),
            usize::from(!hidden)
        );
    }
}

#[test]
fn background_task_keeps_started_row_and_adds_exactly_one_terminal_row() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(tool_call("tool-bg"), Some("p1")));
    let original_id = execute_entries(&projection)[0].0;
    projection.apply(grow_event(GrowUpdate::TaskBackgrounded {
        tool_call_id: "tool-bg".into(),
        task_id: "task-bg".into(),
        command: "sleep 5".into(),
        cwd: "/tmp".into(),
        output_file: "/tmp/task-bg.log".into(),
        monitor_description: None,
        description: Some("background wait".into()),
    }));
    assert_eq!(projection.unresolved_tools(), 0);
    assert!(execute_entries(&projection).is_empty());
    assert!(matches!(
        &projection.scrollback.get_by_id(original_id).unwrap().block,
        RenderBlock::BgTask(_)
    ));
    projection.apply(acp_event(
        tool_terminal("tool-bg", acp::ToolCallStatus::Completed),
        Some("p1"),
    ));
    assert!(execute_entries(&projection).is_empty());
    let start = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
    let end = start + Duration::from_millis(750);
    projection.apply(grow_event(GrowUpdate::TaskCompleted {
        task_snapshot: TaskSnapshot {
            task_id: "task-bg".into(),
            command: "sleep 5".into(),
            display_command: None,
            cwd: "/tmp".into(),
            start_time: start,
            end_time: Some(end),
            output: String::new(),
            output_file: PathBuf::from("/tmp/task-bg.log"),
            truncated: false,
            exit_code: Some(0),
            signal: None,
            completed: true,
            kind: Default::default(),
            block_waited: false,
            explicitly_killed: false,
            owner_session_id: None,
            goal_id: None,
            goal_definition_revision: None,
            description: Some("background wait".into()),
            is_backgrounded: true,
        },
    }));

    let tasks = projection
        .scrollback
        .iter_entries()
        .filter_map(|(_, entry)| match &entry.block {
            RenderBlock::BgTask(task) => Some(task),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].task_id, "task-bg");
    assert!(matches!(&tasks[0].kind, BgTaskKind::Started));
    assert_eq!(tasks[1].task_id, "task-bg");
    assert!(
        matches!(&tasks[1].kind, BgTaskKind::Completed { elapsed } if *elapsed == Duration::from_millis(750))
    );
    assert_eq!(
        tasks
            .iter()
            .filter(|task| matches!(&task.kind, BgTaskKind::Completed { .. }))
            .count(),
        1,
        "the background lifecycle has one terminal row",
    );
}

#[test]
fn hidden_user_echo_separates_prompt_turns_without_leaking_its_body() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(assistant_message("answer p1"), Some("p1")));
    projection.apply(acp_event(
        user_message("private synthetic echo", true),
        Some("p2"),
    ));
    projection.apply(acp_event(assistant_message("answer p2"), Some("p2")));

    let assistant = assistant_entries(&projection);
    assert_eq!(
        assistant.len(),
        2,
        "the hidden prompt still closes the previous turn"
    );
    assert_ne!(assistant[0].0, assistant[1].0);
    assert_eq!(assistant[0].1, "answer p1");
    assert_eq!(assistant[1].1, "answer p2");
    assert_eq!(
        projection
            .scrollback
            .iter_entries()
            .filter(|(_, entry)| matches!(&entry.block, RenderBlock::UserPrompt(_)))
            .count(),
        0,
        "a hidden echo must not create a user row",
    );
    let rendered = markdown(&projection);
    assert!(!rendered.contains("private synthetic echo"));
    assert!(rendered.contains("answer p1"));
    assert!(rendered.contains("answer p2"));
}

#[test]
fn interrupting_parent_message_preserves_body_and_receipt_identity() {
    use shell::extensions::notification::{
        AgentMessageNotice, UiNotice, UiNoticeCategory, UiNoticeTone,
    };
    let notice = UiNotice {
        correlation_id: "receipt-1".into(),
        category: UiNoticeCategory::Coordination,
        subject: Some(AgentMessageNotice::SUBJECT.into()),
        description: None,
        message: "parent sent a message".into(),
        tone: UiNoticeTone::Info,
        details: Some(
            serde_json::to_string(&AgentMessageNotice {
                source_session_id: "parent".into(),
                message_id: "message-1".into(),
                interrupt: true,
                reply_to: None,
                message: Some("stop this path and inspect the failing test".into()),
            })
            .unwrap(),
        ),
    };
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(assistant_message("before "), Some("p1")));
    projection.apply(grow_event(GrowUpdate::UiNotice(notice.clone())));
    projection.apply(grow_event(GrowUpdate::UiNotice(notice)));
    projection.apply(acp_event(assistant_message("after"), Some("p1")));
    assert_eq!(assistant_entries(&projection).len(), 1);
    assert_eq!(assistant_entries(&projection)[0].1, "before after");
    let rendered = markdown(&projection);
    assert_eq!(
        rendered
            .matches("stop this path and inspect the failing test")
            .count(),
        1
    );
    assert!(rendered.contains("Message from parent"));
}

#[path = "behavior_tests.rs"]
mod behavior_tests;

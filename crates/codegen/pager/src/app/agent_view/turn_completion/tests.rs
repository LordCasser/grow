//! Unit tests for the turn-finalize rails in [`super`] (`turn_completion`),
//! split out via `#[path]` to keep the module itself small.

use super::*;
use crate::scrollback::block::RenderBlock;
use crate::scrollback::blocks::SessionEventBlock;
use crate::scrollback::state::ScrollbackState;
use std::path::PathBuf;
use std::time::Instant;

fn last_session_event(sb: &ScrollbackState) -> Option<SessionEvent> {
    (0..sb.len())
        .rev()
        .find_map(|i| match sb.get(i).map(|e| &e.block) {
            Some(RenderBlock::SessionEvent(b)) => Some(b.event.clone()),
            _ => None,
        })
}

/// A viewer in TurnRunning with an adopted prompt id, ready to be finalized.
fn running_viewer(prompt_id: &str) -> AgentView {
    let mut agent = super::super::test_agent_view(Some("s1"), PathBuf::from("/tmp"));
    agent.session.attached_as_viewer = true;
    agent.session.start_turn(&mut agent.scrollback);
    agent.session.current_prompt_id = Some(prompt_id.into());
    agent.session.turn_started_at = Some(Instant::now());
    agent
}

/// A driver in TurnRunning with a local prompt id (default
/// `attached_as_viewer == false`).
fn running_driver(prompt_id: &str) -> AgentView {
    let mut agent = super::super::test_agent_view(Some("s1"), PathBuf::from("/tmp"));
    agent.session.start_turn(&mut agent.scrollback);
    agent.session.current_prompt_id = Some(prompt_id.into());
    agent.session.turn_started_at = Some(Instant::now());
    agent
}

#[test]
fn durable_terminal_immediately_finalizes_driver() {
    let mut agent = running_driver("p1");
    let outcome = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert!(matches!(outcome.apply, TerminalApply::ViewerFinalized));
    assert!(agent.session.state.is_idle());
    assert!(agent.session.current_prompt_id.is_none());
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::TurnCompleted { .. })
    ));
}

#[test]
fn stale_durable_terminal_cannot_finish_new_driver_turn() {
    let mut agent = running_driver("new");
    let outcome = agent.finalize_turn_from_durable_terminal("old", Some("end_turn"), None);
    assert!(matches!(outcome.apply, TerminalApply::Ignored));
    assert!(agent.session.state.is_turn_running());
    assert_eq!(agent.session.current_prompt_id.as_deref(), Some("new"));
    assert!(last_session_event(&agent.scrollback).is_none());
}

#[test]
fn pidless_prompt_response_cannot_finish_running_turn() {
    let mut agent = running_driver("p1");
    let outcome = agent.finalize_prompt_terminal(
        None,
        TerminalMeta {
            pr_ok: true,
            failed_error: None,
            was_cancelling: false,
            bash_turn: false,
            skip_error_marker: false,
            accepts_submitting: true,
        },
    );

    assert!(matches!(outcome.apply, TerminalApply::Ignored));
    assert!(agent.session.state.is_turn_running());
    assert_eq!(agent.session.current_prompt_id.as_deref(), Some("p1"));
}

#[test]
fn viewer_finalize_idles_and_pushes_completed_marker() {
    let mut agent = running_viewer("p1");
    let outcome = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert!(matches!(outcome.apply, TerminalApply::ViewerFinalized));
    assert!(agent.session.state.is_idle());
    assert!(agent.session.current_prompt_id.is_none());
    assert!(agent.session.turn_started_at.is_none());
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::TurnCompleted { .. })
    ));
}

fn one_stop_group() -> Vec<(String, Vec<crate::scrollback::blocks::tool::HookRunEntry>)> {
    use crate::scrollback::blocks::tool::{HookRunEntry, HookRunStatus};
    vec![(
        "stop".to_string(),
        vec![HookRunEntry {
            name: "global/notify".into(),
            status: HookRunStatus::Success {
                elapsed: std::time::Duration::from_millis(12),
            },
            output: None,
        }],
    )]
}

/// Stop-hook groups attached to the last session-event marker.
fn last_marker_groups(sb: &ScrollbackState) -> Option<usize> {
    (0..sb.len())
        .rev()
        .find_map(|i| match sb.get(i).map(|e| &e.block) {
            Some(RenderBlock::SessionEvent(b)) => Some(b.stop_hooks.len()),
            _ => None,
        })
}

fn count_lifecycle_blocks(sb: &ScrollbackState) -> usize {
    use crate::scrollback::blocks::tool::ToolCallBlock;
    (0..sb.len())
        .filter(|i| {
            matches!(
                sb.get(*i).map(|e| &e.block),
                Some(RenderBlock::ToolCall(ToolCallBlock::Lifecycle(_)))
            )
        })
        .count()
}

#[test]
fn marker_push_consumes_matching_stop_hook_stash() {
    let mut agent = running_driver("p1");
    agent.pending_stop_hooks = Some(super::super::PendingStopHooks {
        prompt_id: Some("p1".into()),
        groups: one_stop_group(),
    });

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(2)),
        }),
        Some("p1"),
    );

    assert_eq!(
        last_marker_groups(&agent.scrollback),
        Some(1),
        "the stash must fold into the marker"
    );
    assert!(agent.pending_stop_hooks.is_none());
    assert_eq!(count_lifecycle_blocks(&agent.scrollback), 0);
}

#[test]
fn marker_push_flushes_stale_stash_standalone() {
    // A stash stamped with another turn's prompt id must not attach to
    // this marker — it flushes as the legacy standalone block.
    let mut agent = running_driver("p2");
    agent.pending_stop_hooks = Some(super::super::PendingStopHooks {
        prompt_id: Some("p1".into()),
        groups: one_stop_group(),
    });

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(2)),
        }),
        Some("p2"),
    );

    assert_eq!(
        last_marker_groups(&agent.scrollback),
        Some(0),
        "a stale stash must not attach to the new marker"
    );
    assert_eq!(count_lifecycle_blocks(&agent.scrollback), 1);
    assert!(agent.pending_stop_hooks.is_none());
}

#[test]
fn marker_without_ending_pid_flushes_stamped_stash_standalone() {
    // A stamped stash can't be confirmed against a marker whose ending
    // turn id is missing — it flushes standalone instead of folding into
    // a marker it may not belong to.
    let mut agent = running_driver("p1");
    agent.pending_stop_hooks = Some(super::super::PendingStopHooks {
        prompt_id: Some("p1".into()),
        groups: one_stop_group(),
    });

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(2)),
        }),
        None,
    );

    assert_eq!(
        last_marker_groups(&agent.scrollback),
        Some(0),
        "an unconfirmable stamped stash must not attach to the marker"
    );
    assert_eq!(count_lifecycle_blocks(&agent.scrollback), 1);
    assert!(agent.pending_stop_hooks.is_none());
}

#[test]
fn no_marker_flushes_stash_as_standalone_block() {
    // Turn ends without a marker (bash turn / rate-limit UX): the held
    // hooks still surface, in the legacy standalone form.
    let mut agent = running_driver("p1");
    agent.pending_stop_hooks = Some(super::super::PendingStopHooks {
        prompt_id: Some("p1".into()),
        groups: one_stop_group(),
    });

    agent.push_turn_terminal_marker(None, Some("p1"));

    assert_eq!(count_lifecycle_blocks(&agent.scrollback), 1);
    assert!(agent.pending_stop_hooks.is_none());
}

#[test]
fn viewer_finalize_consumes_stop_hook_stash() {
    // A viewer that stashed hooks mid-turn folds them into the marker the
    // finalize pushes.
    let mut agent = running_viewer("p1");
    agent.pending_stop_hooks = Some(super::super::PendingStopHooks {
        prompt_id: Some("p1".into()),
        groups: one_stop_group(),
    });

    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);

    assert_eq!(last_marker_groups(&agent.scrollback), Some(1));
    assert!(agent.pending_stop_hooks.is_none());
}

#[test]
fn viewer_finalize_duplicate_terminal_is_noop() {
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    let len_after_first = agent.scrollback.len();

    // A duplicate/stale terminal for the now-finished turn does nothing.
    let outcome = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert!(matches!(outcome.apply, TerminalApply::Ignored));
    assert!(agent.session.state.is_idle());
    assert_eq!(
        agent.scrollback.len(),
        len_after_first,
        "a duplicate terminal must not push a second marker"
    );
}

#[test]
fn viewer_finalize_stop_reason_to_marker_mapping() {
    // cancelled → Turn cancelled.
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("cancelled"), None);
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::TurnCancelled { .. })
    ));

    // error (+agentResult) → Turn failed carrying the error text.
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("error"), Some("boom"));
    match last_session_event(&agent.scrollback) {
        Some(SessionEvent::TurnFailed { error, .. }) => assert_eq!(error, "boom"),
        other => panic!("expected TurnFailed, got {other:?}"),
    }

    // rate_limit → finished, but no marker (not actionable from a viewer).
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("rate_limit"), None);
    assert!(agent.session.state.is_idle());
    assert!(
        last_session_event(&agent.scrollback).is_none(),
        "rate_limit must not push a marker on a viewer"
    );

    // unknown/other reason → Turn completed (the catch-all).
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("max_tokens"), None);
    assert!(matches!(
        last_session_event(&agent.scrollback),
        Some(SessionEvent::TurnCompleted { .. })
    ));
}

// ── End markers: always the plain event text (work lives in the status row) ──

fn insert_bg_task(agent: &mut AgentView, task_id: &str, is_monitor: bool) {
    agent.session.bg_tasks.insert(
        task_id.into(),
        crate::app::session::BgTaskState {
            task_id: task_id.into(),
            tool_call_id: format!("call-{task_id}"),
            command: "sleep 5".into(),
            description: None,
            cwd: "/tmp".into(),
            output_file: "/tmp/out".into(),
            status: crate::app::session::BgTaskStatus::Running,
            start_time: std::time::SystemTime::now(),
            end_time: None,
            exit_code: None,
            signal: None,
            stdout: String::new(),
            stdout_line_count: 0,
            truncated: false,
            pending_kill: false,
            kill_requested_at: None,
            scrollback_entry_id: None,
            is_monitor,
            restored_from_replay: false,
        },
    );
}

/// The newest session-event marker block.
fn last_marker_block(agent: &AgentView) -> &SessionEventBlock {
    (0..agent.scrollback.len())
        .rev()
        .find_map(|i| match agent.scrollback.get(i).map(|e| &e.block) {
            Some(RenderBlock::SessionEvent(b)) => Some(b),
            _ => None,
        })
        .expect("a session-event marker must exist")
}

#[test]
fn real_end_marker_stays_plain_with_running_work() {
    let mut agent = running_driver("p1");
    insert_bg_task(&mut agent, "bg-1", false);

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(2)),
        }),
        Some("p1"),
    );

    let block = last_marker_block(&agent);
    assert_eq!(block.prompt_id.as_deref(), Some("p1"));
    assert_eq!(block.event.message(), "Worked for 2.0s");
    assert_eq!(
        agent.watchers().commands,
        1,
        "the running command feeds the status-row watchers cue instead"
    );
}

#[test]
fn workless_marker_renders_legacy_text() {
    let mut agent = running_driver("p1");

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(2)),
        }),
        Some("p1"),
    );

    let block = last_marker_block(&agent);
    assert_eq!(block.event.message(), "Worked for 2.0s");
}

/// The turn-end marker takes no fold path — a park has no row to fold into.
#[test]
fn turn_end_after_park_pushes_single_marker() {
    use crate::app::agent_view::test_fixtures::count_turn_markers;

    let mut agent = running_driver("p1");
    super::super::test_fixtures::simulate_task_output_wait(&mut agent, "bg-1");
    assert!(agent.renders_parked());
    assert_eq!(count_turn_markers(&agent), 0, "the park writes no marker");

    agent.push_turn_terminal_marker(
        Some(SessionEvent::TurnCompleted {
            elapsed: Some(std::time::Duration::from_secs(5)),
        }),
        Some("p1"),
    );

    assert_eq!(
        count_turn_markers(&agent),
        1,
        "the real turn end pushes exactly one marker"
    );
    assert_eq!(last_marker_block(&agent).event.message(), "Worked for 5.0s");
}

// ── Shared finalizer: full teardown, exactly once ──────────────────

/// Push one synthetic permission request; returns the response receiver so
/// the test can assert the drain actually answered.
fn push_synthetic_permission(
    agent: &mut AgentView,
    id: usize,
) -> tokio::sync::oneshot::Receiver<Result<acp::RequestPermissionResponse, acp::Error>> {
    use crate::views::permission_view::{PermissionFocus, PermissionViewState};
    let (tx, rx) =
        tokio::sync::oneshot::channel::<Result<acp::RequestPermissionResponse, acp::Error>>();
    let request = acp_transport::AcpArgs {
        request: acp::RequestPermissionRequest::new(
            agent
                .session
                .session_id
                .clone()
                .expect("synthetic permission requires a session id"),
            acp::ToolCallUpdate::new(
                acp::ToolCallId::new(std::sync::Arc::from("tc-1")),
                acp::ToolCallUpdateFields::default(),
            ),
            vec![acp::PermissionOption::new(
                acp::PermissionOptionId::new(std::sync::Arc::from("allow")),
                "Allow",
                acp::PermissionOptionKind::AllowOnce,
            )],
        ),
        response_tx: tx,
    };
    let options = request.request.options.clone();
    let state = PermissionViewState {
        request,
        id,
        focus: PermissionFocus::Options,
        options,
        active_idx: 0,
        bash_highlights: None,
        bash_selection_count: 0,
        bash_command_raw: None,
        mcp_scope: None,
        title: "test permission".to_string(),
        description: Vec::new(),
        args_expanded: false,
        desc_scroll: 0,
        subagent_label: None,
        options_area_height: 0,
        options_scroll_offset: 0,
    };
    agent.push_permission(state);
    rx
}

/// The winner of the first-wins gate runs the FULL turn-end teardown that
/// the PromptResponse rail used to own — permission queue drain, plan
/// approval dismissal, cancel-panel cleanup, bash wrap-up, cron id,
/// prompt-suggestion wipe — plus the finalized marker. A duplicate terminal
/// must not repeat any of it.
#[test]
fn viewer_finalize_runs_full_teardown_once() {
    use crate::views::modal::CancelTurnViewState;
    use crate::views::plan_approval_view::PlanApprovalViewState;
    use tools::implementations::grow_build::plan_control::PlanApprovalExtRequest;

    let mut agent = running_viewer("p1");
    let mut perm_rx = push_synthetic_permission(&mut agent, 1);
    let (plan_tx, mut plan_rx) = tokio::sync::oneshot::channel();
    agent.plan_approval_view = Some(PlanApprovalViewState::new(
        PlanApprovalExtRequest {
            session_id: "s1".into(),
            tool_call_id: "plan-1".into(),
            plan_content: "plan text".into(),
        },
        Default::default(),
        plan_tx,
    ));
    agent.cancel_turn_view = Some(CancelTurnViewState {
        active_idx: 0,
        running_count: 2,
    });
    agent
        .cancel_turn_buttons
        .push(ratatui::layout::Rect::default());
    agent.bash_turn = true;
    agent.session.activity_started_at = Some(Instant::now());
    agent.session.last_activity = Some(crate::acp::tracker::TurnActivity::Thinking);
    let generation = agent.prompt.prompt_suggestion.begin_fetch();
    assert!(
        agent
            .prompt
            .prompt_suggestion
            .on_loaded(Some("next prompt".into()), generation),
        "suggestion must be installed before the finalize"
    );

    let outcome = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert!(matches!(outcome.apply, TerminalApply::ViewerFinalized));

    assert!(agent.permission_queue.is_empty(), "permissions drained");
    assert!(
        matches!(
            perm_rx.try_recv(),
            Ok(Ok(acp::RequestPermissionResponse { .. }))
        ),
        "drained permissions answered Cancelled"
    );
    assert!(
        plan_rx.try_recv().is_ok(),
        "the dismissed plan approval answered stale-cancel"
    );
    assert!(
        agent.plan_approval_view.is_none(),
        "plan approval dismissed"
    );
    assert!(agent.cancel_turn_view.is_none(), "cancel panel cleared");
    assert!(
        agent.cancel_turn_buttons.is_empty(),
        "cancel buttons cleared"
    );
    assert!(!agent.bash_turn, "bash turn flag reset");
    assert!(agent.session.activity_started_at.is_none());
    assert!(agent.session.last_activity.is_none());
    assert!(
        !agent.prompt.prompt_suggestion.has_suggestion(),
        "stale prompt suggestion wiped"
    );
    assert_eq!(agent.session.finalized_prompt.as_deref(), Some("p1"));

    // The duplicate/stale terminal must not repeat any of the teardown.
    let duplicate = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert!(matches!(duplicate.apply, TerminalApply::Ignored));
    assert_eq!(agent.session.finalized_prompt.as_deref(), Some("p1"));
    assert!(agent.permission_queue.is_empty());
    assert!(agent.plan_approval_view.is_none());
    assert!(!agent.bash_turn);
}

// ── Late PromptResponse merge (durable rail won first) ─────────────

/// A durable win records the winner; a late PromptResponse for the same pid
/// merges usage / structured output / error details and nothing else.
#[test]
fn late_prompt_response_merges_usage_and_structured_output_only() {
    let mut agent = running_viewer("p1");
    let _ = agent.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    assert_eq!(agent.session.finalized_prompt.as_deref(), Some("p1"));

    let mut meta = serde_json::Map::new();
    meta.insert("promptId".into(), serde_json::json!("p1"));
    meta.insert("structuredOutput".into(), serde_json::json!({"ok": true}));
    meta.insert(
        "usage".into(),
        serde_json::json!({"totalTokens": 42, "inputTokens": 10, "outputTokens": 32}),
    );
    let pr = acp::PromptResponse::new(acp::StopReason::EndTurn).meta(meta);
    agent.merge_finalized_pr_meta(&Ok(pr));

    let merged = agent
        .session
        .finalized_pr_meta
        .as_ref()
        .expect("meta merged");
    assert_eq!(
        merged.usage.as_ref().map(|u| u.totals.total_tokens),
        Some(42),
        "late usage merged"
    );
    assert!(
        matches!(merged.structured_output.as_ref(), Some(Ok(v)) if v == &serde_json::json!({"ok": true})),
        "late structured output merged"
    );
    assert!(merged.error.is_none());

    // A second late response fills only the missing fields (no clobber).
    agent.merge_finalized_pr_meta(&Err("late boom".to_string()));
    let merged = agent.session.finalized_pr_meta.as_ref().unwrap();
    assert_eq!(merged.error.as_deref(), Some("late boom"));
    assert_eq!(
        merged.usage.as_ref().map(|u| u.totals.total_tokens),
        Some(42),
        "usage must survive a later error response"
    );
    assert!(
        matches!(merged.structured_output.as_ref(), Some(Ok(v)) if v == &serde_json::json!({"ok": true})),
        "structured output must survive a later error response"
    );

    // The structured-output-error form maps to Err.
    let mut agent2 = running_viewer("p1");
    let _ = agent2.finalize_turn_from_durable_terminal("p1", Some("end_turn"), None);
    let mut meta = serde_json::Map::new();
    meta.insert("promptId".into(), serde_json::json!("p1"));
    meta.insert(
        "structuredOutputError".into(),
        serde_json::json!("schema mismatch"),
    );
    agent2.merge_finalized_pr_meta(&Ok(
        acp::PromptResponse::new(acp::StopReason::EndTurn).meta(meta)
    ));
    assert!(
        matches!(
            agent2.session.finalized_pr_meta.as_ref().unwrap().structured_output.as_ref(),
            Some(Err(e)) if e == "schema mismatch"
        ),
        "structuredOutputError maps to the Err form"
    );
}

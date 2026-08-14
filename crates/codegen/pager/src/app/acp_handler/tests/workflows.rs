#![cfg_attr(rustfmt, rustfmt::skip)]
use super::*;
use crate::scrollback::blocks::{WorkflowBlock, WorkflowBlockStatus};

/// Serialize a `WorkflowUpdated` session notification and dispatch it through
/// the real entry: AppView → `handle_ext_notification` →
/// `handle_session_notification` → `ingest_workflow_update`.
fn send_workflow_update(
    app: &mut AppView,
    update: serde_json::Value,
    meta: Option<serde_json::Value>,
) -> bool {
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": update,
        "_meta": meta,
    });
    let raw = serde_json::value::to_raw_value(&payload).unwrap();
    let (tx, _rx) = tokio::sync::oneshot::channel();
    handle(
        AcpClientMessage::ExtNotification(acp_transport::AcpArgs {
            request: acp::ExtNotification::new("grow/session_notification", raw.into()),
            response_tx: tx,
        }),
        app,
    )
}

/// Default active private deep-research update; `overrides` are shallow-merged
/// on top.
fn private_workflow_update(overrides: serde_json::Value) -> serde_json::Value {
    let mut update = serde_json::json!({
        "sessionUpdate": "workflow_updated",
        "run_id": "wf_deep",
        "private": true,
        "definition_id": null,
        "definition_scope": null,
        "definition_hash": null,
        "revision": 1,
        "name": "deep-research",
        "objective": "investigate X",
        "status": "active",
        "foreground": false,
        "phases": [{"title": "Research", "state": "active"}],
        "current_phase": "Research",
        "agent_budget": 8,
        "agents_used": 1,
        "agents_reserved": 1,
        "agents_remaining": 7,
        "agent_usage_incomplete": false,
        "elapsed_ms": 12000,
        "active_agents": 1,
        "current_agent_label": "researcher-0",
        "agents": [
            {
                "agent_id": "a1",
                "label": "researcher-0",
                "phase": "Research",
                "model": null,
                "state": "running",
                "tokens_used": 100,
                "duration_ms": 5000,
            }
        ],
        "last_event": null,
        "last_event_detail": null,
        "last_event_timestamp": null,
        "pause_message": null,
        "result_summary": null,
    });
    for (key, value) in overrides.as_object().unwrap() {
        update.as_object_mut().unwrap().insert(key.clone(), value.clone());
    }
    update
}

/// Resolve the live block through the `workflow_blocks` map (running runs).
fn mapped_workflow_block<'a>(agent: &'a AgentView, run_id: &str) -> &'a WorkflowBlock {
    let id = agent
        .workflow_blocks
        .get(run_id)
        .expect("run must map to a transcript block");
    let entry = agent
        .scrollback
        .get_by_id(*id)
        .expect("mapped entry must exist in the scrollback");
    match &entry.block {
        RenderBlock::Workflow(wb) => wb,
        other => panic!("expected a Workflow block, got {other:?}"),
    }
}

/// Find the run's block in the transcript by run id (works after terminal
/// updates release the `workflow_blocks` mapping).
fn transcript_workflow_block<'a>(agent: &'a AgentView, run_id: &str) -> &'a WorkflowBlock {
    for idx in 0..agent.scrollback.len() {
        if let Some(entry) = agent.scrollback.get(idx)
            && let RenderBlock::Workflow(wb) = &entry.block
            && wb.run_id == run_id
        {
            return wb;
        }
    }
    panic!("no transcript Workflow block for run {run_id}");
}

#[test]
fn private_active_update_creates_progress_block_and_isolates_from_public_lists() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();

    // Management isolation is structural: nothing lands in workflow_runs.
    assert!(
        agent.workflow_runs.is_empty(),
        "a private run must never enter the public workflow_runs list"
    );
    assert_eq!(agent.private_workflow_runs.len(), 1);
    let run = &agent.private_workflow_runs[0];
    assert_eq!(run.run_id, "wf_deep");
    assert_eq!(run.name, "deep-research");
    assert_eq!(run.status, "active");
    assert!(
        !run.management_available,
        "private snapshots must never be manageable"
    );
    assert_eq!(run.current_phase.as_deref(), Some("Research"));

    // Transcript progress block carries live phase/agent/elapsed semantics.
    let wb = mapped_workflow_block(agent, "wf_deep");
    assert!(
        matches!(wb.status, WorkflowBlockStatus::Running),
        "block must run while the update is active: {:?}",
        wb.status
    );
    assert_eq!(wb.current_phase.as_deref(), Some("Research"));
    assert_eq!(wb.active_agents, 1);
    assert_eq!(wb.phases.len(), 1);
    assert_eq!(wb.phases[0].title, "Research");
    assert_eq!(wb.phases[0].state, "active");
    assert_eq!(wb.elapsed, std::time::Duration::from_millis(12000));
}

#[test]
fn private_phase_update_refreshes_block_and_replaces_snapshot() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({
            "revision": 2,
            "phases": [
                {"title": "Research", "state": "done"},
                {"title": "Verify", "state": "active"},
            ],
            "current_phase": "Verify",
            "active_agents": 2,
            "elapsed_ms": 45000,
            "agents": [
                {
                    "agent_id": "a1",
                    "label": "researcher-0",
                    "phase": "Research",
                    "model": null,
                    "state": "done",
                    "tokens_used": 500,
                    "duration_ms": 30000,
                },
                {
                    "agent_id": "a2",
                    "label": "evidence-verifier-0",
                    "phase": "Verify",
                    "model": null,
                    "state": "running",
                    "tokens_used": 50,
                    "duration_ms": 8000,
                },
            ],
        })),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();

    assert!(agent.workflow_runs.is_empty());
    assert_eq!(
        agent.private_workflow_runs.len(),
        1,
        "same run must be upserted, not duplicated"
    );
    let run = &agent.private_workflow_runs[0];
    assert_eq!(run.current_phase.as_deref(), Some("Verify"));
    assert_eq!(run.elapsed_ms, 45000);

    let wb = mapped_workflow_block(agent, "wf_deep");
    assert!(matches!(wb.status, WorkflowBlockStatus::Running));
    assert_eq!(wb.current_phase.as_deref(), Some("Verify"));
    assert_eq!(
        wb.active_agents, 1,
        "only agents in state \"running\" count as active"
    );
    assert_eq!(wb.phases.len(), 2);
    assert_eq!(wb.phases[0].title, "Research");
    assert_eq!(wb.phases[0].state, "done");
    assert_eq!(wb.phases[1].title, "Verify");
    assert_eq!(wb.phases[1].state, "active");
    assert_eq!(wb.elapsed, std::time::Duration::from_millis(45000));
}

#[test]
fn private_terminal_converges_block_and_removes_private_entry() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({
            "revision": 3,
            "status": "complete",
            "elapsed_ms": 60000,
            "agents": [],
        })),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();

    assert!(
        agent.private_workflow_runs.is_empty(),
        "terminal private runs must not linger in the tasks pane"
    );
    assert!(
        !agent.workflow_blocks.contains_key("wf_deep"),
        "a terminal block must release its run-id mapping"
    );
    // The block itself remains in the transcript, converged to a terminal
    // ("done in X") and no longer running.
    let wb = transcript_workflow_block(agent, "wf_deep");
    assert!(
        matches!(wb.status, WorkflowBlockStatus::Done { .. }),
        "complete must converge the block to Done: {:?}",
        wb.status
    );
}

#[test]
fn private_budget_limited_converges_block_to_failed_and_clears_entry() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({
            "revision": 3,
            "status": "budget_limited",
            "elapsed_ms": 90000,
            "agents": [],
        })),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.private_workflow_runs.is_empty());
    assert!(!agent.workflow_blocks.contains_key("wf_deep"));
    let wb = transcript_workflow_block(agent, "wf_deep");
    assert!(
        matches!(wb.status, WorkflowBlockStatus::Failed { .. }),
        "budget_limited must converge the private block to a terminal state, got {:?}",
        wb.status
    );
}

#[test]
fn private_cleared_removes_block_collection_and_guards_revision_zero() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({
            "revision": 4,
            "status": "cleared",
            "elapsed_ms": 70000,
        })),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(agent.private_workflow_runs.is_empty());
    assert!(!agent.workflow_blocks.contains_key("wf_deep"));
    assert!(
        agent.cleared_workflow_runs.contains("wf_deep"),
        "cleared guard must remember the run id"
    );

    // A late revision-0 re-emission must be dropped like the public path.
    assert!(!send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({"revision": 0})),
        None,
    ));
    assert!(
        app.agents
            .get(&AgentId(0))
            .unwrap()
            .private_workflow_runs
            .is_empty()
    );
}

#[test]
fn private_revision_dedup_matches_public_semantics() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    // Same revision re-emitted → dropped.
    assert!(!send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        None,
    ));
    // Lower revision → dropped.
    assert!(!send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({
            "revision": 0,
            "current_phase": "Stale",
        })),
        None,
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert_eq!(agent.private_workflow_runs.len(), 1);
    assert_eq!(
        agent.private_workflow_runs[0].current_phase.as_deref(),
        Some("Research"),
        "stale revisions must not overwrite the snapshot"
    );
}

#[test]
fn replay_rebuilds_the_same_private_progress_block() {
    let mut app = make_app_with_agent("sess-A");
    app.agents
        .get_mut(&AgentId(0))
        .unwrap()
        .session
        .loading_replay = true;
    assert!(send_workflow_update(
        &mut app,
        private_workflow_update(serde_json::json!({})),
        Some(serde_json::json!({ "isReplay": true, "eventId": "sess-A-9" })),
    ));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert!(
        agent.workflow_runs.is_empty(),
        "replayed private run must stay out of public lists"
    );
    assert_eq!(agent.private_workflow_runs.len(), 1);
    let wb = mapped_workflow_block(agent, "wf_deep");
    assert!(matches!(wb.status, WorkflowBlockStatus::Running));
    assert_eq!(wb.current_phase.as_deref(), Some("Research"));
    assert_eq!(wb.active_agents, 1);
    assert_eq!(wb.elapsed, std::time::Duration::from_millis(12000));
}

#[test]
fn public_runs_still_ingest_into_workflow_runs_and_blocks() {
    let mut app = make_app_with_agent("sess-A");
    let mut update = private_workflow_update(serde_json::json!({}));
    update
        .as_object_mut()
        .unwrap()
        .insert("private".into(), false.into());
    assert!(send_workflow_update(&mut app, update, None));
    let agent = app.agents.get(&AgentId(0)).unwrap();
    assert_eq!(
        agent.workflow_runs.len(),
        1,
        "public runs must keep landing in workflow_runs"
    );
    assert!(agent.private_workflow_runs.is_empty());
    assert_eq!(agent.workflow_runs[0].name, "deep-research");
    assert!(agent.workflow_blocks.contains_key("wf_deep"));
    let wb = mapped_workflow_block(agent, "wf_deep");
    assert!(matches!(wb.status, WorkflowBlockStatus::Running));
}

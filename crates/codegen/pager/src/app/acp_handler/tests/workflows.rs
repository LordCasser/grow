use super::*;

fn send_workflow_update(app: &mut AppView, update: serde_json::Value) -> bool {
    let payload = serde_json::json!({
        "sessionId": "sess-A",
        "update": update,
        "_meta": null,
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

fn workflow_update() -> serde_json::Value {
    serde_json::json!({
        "sessionUpdate": "workflow_updated",
        "run_id": "wf_deep",
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
        "agents_remaining": 7,
        "agent_usage_incomplete": false,
        "elapsed_ms": 12000,
        "active_agents": 1,
        "current_agent_label": "researcher-0",
        "agents": [{
            "agent_id": "a1",
            "label": "researcher-0",
            "phase": "Research",
            "model": null,
            "state": "running",
            "tokens_used": 100,
            "duration_ms": 5000
        }],
        "pause_message": null,
        "result_summary": null
    })
}

#[test]
fn workflow_updates_use_the_common_public_projection() {
    let mut app = make_app_with_agent("sess-A");
    assert!(send_workflow_update(&mut app, workflow_update()));
    let agent = app.agents.get(&AgentId(0)).unwrap();

    assert_eq!(agent.session.workflow_runs.len(), 1);
    let run = &agent.session.workflow_runs[0];
    assert_eq!(run.name, "deep-research");
    assert_eq!(run.status, "active");
    assert_eq!(run.current_phase.as_deref(), Some("Research"));
    assert!(agent.workflow_blocks.contains_key("wf_deep"));

    let block_id = agent.workflow_blocks["wf_deep"];
    let entry = agent.scrollback.get_by_id(block_id).unwrap();
    let RenderBlock::Workflow(block) = &entry.block else {
        panic!("expected a workflow transcript block")
    };
    assert_eq!(block.current_phase.as_deref(), Some("Research"));
    assert_eq!(block.active_agents, 1);
}

use super::*;

fn goal_update(
    goal_id: &str,
    objective: &str,
    status: &str,
    tokens_used: i64,
    token_budget: Option<i64>,
) -> GrowUpdate {
    serde_json::from_value(serde_json::json!({
        "sessionUpdate": "goal_updated",
        "goal_id": goal_id,
        "objective": objective,
        "status": status,
        "token_budget": token_budget,
        "tokens_used": tokens_used,
        "usage_incomplete": true,
        "elapsed_ms": 1200,
        "created_at": "2026-09-29T00:00:00Z",
        "updated_at": "2026-09-29T00:01:00Z"
    }))
    .expect("valid GoalUpdated fixture")
}

fn workflow_update(run_id: &str, revision: u64, status: &str, objective: &str) -> GrowUpdate {
    serde_json::from_value(serde_json::json!({
        "sessionUpdate": "workflow_updated",
        "run_id": run_id,
        "revision": revision,
        "name": "Release workflow",
        "objective": objective,
        "status": status,
        "elapsed_ms": 2400,
        "phases": [
            { "title": "inspect", "state": "done" },
            { "title": "implement", "state": "active" }
        ]
    }))
    .expect("valid WorkflowUpdated fixture")
}

fn goal_notices(projection: &TranscriptProjection) -> Vec<(EntryId, String)> {
    projection
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| match &entry.block {
            RenderBlock::Notice(notice) if notice.text.starts_with("Goal ·") => {
                Some((id, notice.text.clone()))
            }
            _ => None,
        })
        .collect()
}

fn workflow_blocks(projection: &TranscriptProjection) -> Vec<(EntryId, WorkflowBlock)> {
    projection
        .scrollback
        .iter_entries()
        .filter_map(|(id, entry)| match &entry.block {
            RenderBlock::Workflow(workflow) => Some((id, workflow.clone())),
            _ => None,
        })
        .collect()
}

fn current_mode_update(
    mode: &str,
    plan_phase: Option<&str>,
    change: Option<serde_json::Value>,
) -> acp::SessionUpdate {
    let mut meta = acp::Meta::new();
    if let Some(phase) = plan_phase {
        meta.insert("grow/planPhase".into(), serde_json::json!(phase));
    }
    if let Some(change) = change {
        meta.insert("grow/behaviorChange".into(), change);
    }
    acp::SessionUpdate::CurrentModeUpdate(
        acp::CurrentModeUpdate::new(acp::SessionModeId::new(mode)).meta(meta),
    )
}

#[test]
fn goal_statuses_and_late_usage_keep_the_latest_state_without_inventing_a_budget() {
    for status in ["paused", "blocked", "budget_limited", "complete"] {
        let mut projection = TranscriptProjection::default();
        projection.apply(grow_event(goal_update(
            "g1",
            "ship release",
            "active",
            10,
            None,
        )));
        projection.apply(grow_event(goal_update(
            "g1",
            "ship release",
            status,
            40,
            None,
        )));
        // A later usage update for the same terminal state changes the usage,
        // while preserving that state and the original notice identity.
        projection.apply(grow_event(goal_update(
            "g1",
            "ship release",
            status,
            1234,
            None,
        )));

        let notices = goal_notices(&projection);
        assert_eq!(notices.len(), 1, "status {status} must stay one Goal row");
        let display_status = status.replace('_', " ");
        assert!(
            notices[0].1.contains(status) || notices[0].1.contains(&display_status),
            "latest status {status}"
        );
        assert!(notices[0].1.contains(">=1234 tokens"), "latest usage");
        assert!(notices[0].1.contains("no token budget"));
        assert_eq!(projection.goal_status(), Some(status));
    }
}

#[test]
fn goal_transcript_details_keep_cache_rate_and_partial_qualifier() {
    let mut projection = TranscriptProjection::default();
    for (unclassified, tokens_used, expected) in [
        (0, 580, "Cache hit rate: 40.00%"),
        (500, 1_080, "Cache hit rate: measured 40.00%"),
    ] {
        let update: GrowUpdate = serde_json::from_value(serde_json::json!({
            "sessionUpdate": "goal_updated",
            "goal_id": "g1",
            "objective": "ship release",
            "status": "active",
            "token_budget": null,
            "tokens_used": tokens_used,
            "usage_breakdown": {
                "cached_input_tokens": 200,
                "uncached_input_tokens": 300,
                "unclassified_input_tokens": unclassified,
                "output_tokens": 80
            },
            "usage_incomplete": false,
            "elapsed_ms": 1200,
            "created_at": "2026-09-29T00:00:00Z",
            "updated_at": "2026-09-29T00:01:00Z"
        }))
        .unwrap();
        projection.apply(grow_event(update));
        let details = projection
            .scrollback
            .iter_entries()
            .find_map(|(_, entry)| match &entry.block {
                RenderBlock::Notice(notice) if notice.text.starts_with("Goal ·") => {
                    notice.details.as_deref()
                }
                _ => None,
            })
            .expect("Goal details should be projected");
        assert!(details.contains(expected), "{details}");
    }
}

#[test]
fn empty_id_clear_removes_goal_and_stale_updates_cannot_restore_it() {
    let mut projection = TranscriptProjection::default();
    projection.apply(grow_event(goal_update(
        "old-goal",
        "old objective",
        "active",
        3,
        None,
    )));
    let cleared: GrowUpdate = serde_json::from_value(serde_json::json!({
        "sessionUpdate": "goal_updated",
        "goal_id": "",
        "objective": "",
        "status": "cleared",
        "token_budget": null,
        "tokens_used": 0,
        "elapsed_ms": 0,
        "created_at": "",
        "updated_at": ""
    }))
    .expect("valid cleared Goal fixture");
    projection.apply(grow_event(cleared));
    assert!(
        goal_notices(&projection).is_empty(),
        "clear removes the old row"
    );
    assert_eq!(projection.goal_status(), None);

    projection.apply(grow_event(goal_update(
        "old-goal",
        "old objective",
        "active",
        8,
        None,
    )));
    assert!(
        goal_notices(&projection).is_empty(),
        "late old state stays cleared"
    );

    projection.apply(grow_event(goal_update(
        "new-goal",
        "new objective",
        "active",
        1,
        None,
    )));
    let rendered = markdown(&projection);
    assert!(!rendered.contains("old objective"));
    assert!(rendered.contains("new objective"));
    assert_eq!(projection.goal_status(), Some("active"));
}

#[test]
fn paused_goal_with_same_id_can_restart() {
    let mut projection = TranscriptProjection::default();
    projection.apply(grow_event(goal_update(
        "g1",
        "ship release",
        "paused",
        20,
        Some(5000),
    )));
    projection.apply(grow_event(goal_update(
        "g1",
        "ship release",
        "active",
        21,
        Some(5000),
    )));

    let notices = goal_notices(&projection);
    assert_eq!(notices.len(), 1);
    assert!(notices[0].1.contains("active"));
    assert!(!notices[0].1.contains("paused"));
}

#[test]
fn behavior_and_goal_events_do_not_split_an_assistant_message_or_merge_the_next_prompt() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(assistant_message("A"), Some("p1")));
    let first_id = assistant_entries(&projection)[0].0;
    projection.apply(acp_event(
        current_mode_update("goal", None, None),
        Some("p1"),
    ));
    projection.apply(grow_event(goal_update(
        "g1",
        "ship release",
        "active",
        2,
        None,
    )));
    projection.apply(turn_completed("older-prompt", "cancelled"));
    projection.apply(acp_event(
        current_mode_update("normal", None, None),
        Some("p1"),
    ));
    projection.apply(acp_event(assistant_message("B"), Some("p1")));

    let assistant = assistant_entries(&projection);
    assert_eq!(assistant.len(), 1);
    assert_eq!(assistant[0], (first_id, "AB".to_owned()));

    projection.apply(acp_event(user_message("next prompt", false), Some("p2")));
    projection.apply(acp_event(assistant_message("C"), Some("p2")));
    let assistant = assistant_entries(&projection);
    assert_eq!(assistant.len(), 2);
    assert_eq!(assistant[0], (first_id, "AB".to_owned()));
    assert_eq!(assistant[1].1, "C");
    assert_ne!(assistant[1].0, first_id);
}

#[test]
fn plan_mode_keeps_current_behavior_and_all_phases_without_auto_approval() {
    let mut projection = TranscriptProjection::default();
    for phase in ["drafting", "awaiting_approval", "executing", "amending"] {
        projection.apply(acp_event(
            current_mode_update("plan", Some(phase), None),
            None,
        ));
        let label = projection.behavior_label().expect("Plan is displayed");
        assert!(label.contains("plan"));
        let display_phase = phase.replace('_', " ");
        assert!(
            label.contains(phase) || label.contains(&display_phase),
            "phase {phase} is visible in {label}"
        );
    }

    projection.apply(acp_event(
        current_mode_update(
            "plan",
            Some("awaiting_approval"),
            Some(serde_json::json!({
                "status": "confirmation_required",
                "source": "plan",
                "target": "normal",
                "message": "Select again to confirm leaving Plan."
            })),
        ),
        None,
    ));
    let label = projection.behavior_label().expect("Plan remains current");
    assert!(label.contains("plan"));
    assert!(label.contains("awaiting_approval"));
    assert!(
        !label.contains("normal"),
        "requested target is not current mode"
    );
    assert!(!markdown(&projection).contains("Behavior applied"));
}

#[test]
fn workflow_revision_guard_survives_mode_changes_and_clear() {
    let mut projection = TranscriptProjection::default();
    projection.apply(grow_event(workflow_update(
        "run-1",
        2,
        "active",
        "new objective",
    )));
    projection.apply(grow_event(workflow_update("run-1", 1, "cleared", "")));
    assert!(projection.scrollback.has_running_entries());
    projection.apply(grow_event(workflow_update(
        "run-1",
        3,
        "complete",
        "new objective",
    )));
    projection.apply(grow_event(workflow_update(
        "run-1",
        1,
        "active",
        "stale objective",
    )));
    projection.apply(acp_event(current_mode_update("workflow", None, None), None));
    projection.apply(acp_event(current_mode_update("normal", None, None), None));

    let workflows = workflow_blocks(&projection);
    assert_eq!(workflows.len(), 1);
    assert!(matches!(
        workflows[0].1.status,
        WorkflowBlockStatus::Done { .. }
    ));
    assert_eq!(workflows[0].1.objective, "new objective");
    assert!(markdown(&projection).contains("inspect"));
    assert!(markdown(&projection).contains("implement"));

    projection.apply(grow_event(workflow_update("run-1", 4, "cleared", "")));
    projection.apply(grow_event(workflow_update(
        "run-1",
        4,
        "active",
        "same revision",
    )));
    projection.apply(grow_event(workflow_update(
        "run-1",
        2,
        "active",
        "older revision",
    )));
    projection.apply(grow_event(workflow_update(
        "run-1",
        0,
        "active",
        "unversioned stale update",
    )));
    let after_clear = workflow_blocks(&projection);
    assert_eq!(after_clear.len(), 1, "clear retains the historical run row");
    assert_eq!(after_clear[0].0, workflows[0].0);
    assert!(matches!(
        after_clear[0].1.status,
        WorkflowBlockStatus::Done { .. }
    ));
    assert_eq!(after_clear[0].1.objective, "new objective");
}

#[test]
fn control_receipts_are_observational_and_deduplicated() {
    let mut projection = TranscriptProjection::default();
    projection.apply(acp_event(
        current_mode_update("plan", Some("awaiting_approval"), None),
        None,
    ));
    for (revision, phase) in [
        (1, "pending"),
        (2, "applying"),
        (3, "superseded"),
        (4, "rejected"),
        (5, "applied"),
    ] {
        let update: GrowUpdate = serde_json::from_value(serde_json::json!({
            "sessionUpdate": "control_state_update",
            "epoch": "actor-1", "domain": "behavior", "revision": revision,
            "phase": phase,
            "current": { "kind": "behavior", "behavior_id": "plan" },
            "desired": { "kind": "behavior", "behavior_id": "normal" },
            "message": format!("recorded {phase} receipt")
        }))
        .unwrap();
        projection.apply(grow_event(update.clone()));
        projection.apply(grow_event(update));
    }
    let text = markdown(&projection);
    for phase in ["pending", "applying", "superseded"] {
        assert!(!text.contains(&format!("recorded {phase} receipt")));
    }
    for phase in ["rejected", "applied"] {
        assert_eq!(
            text.matches(&format!("recorded {phase} receipt")).count(),
            1
        );
    }
    assert_eq!(
        projection.behavior_label().as_deref(),
        Some("plan / awaiting_approval")
    );
}

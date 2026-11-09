use super::*;
use tool_types::planning::{PlanDelegation, PlanPriority, ReplanOutcome};

const CHILD: &str = "actual-child-B";
const READ_ALIAS: &str = "host_7f3_read_plan";

fn step(index: usize) -> PlanStep {
    PlanStep {
        id: format!("step-{index}"),
        content: format!("private sibling body {index}"),
        priority: PlanPriority::High,
        status: PlanStepStatus::InProgress,
        requirements: None,
        removed: false,
    }
}

fn parent_fixture(count: usize) -> (ParentPlanBinding, PlanningReadSnapshot) {
    let position = count - 1;
    let binding = ParentPlanBinding {
        parent_session_id: "direct-parent-A".into(),
        task_id: "this-execution".into(),
        parent_step_id: format!("step-{position}"),
    };
    let mut plan = LocalPlan::new(&binding.parent_session_id, "scope:direct-parent");
    plan.revision = 20;
    plan.steps = (0..count).map(step).collect();
    plan.steps[position].content = "The current delegated objective".into();
    plan.steps[position].requirements = Some(vec!["Use the actual direct parent".into()]);
    plan.links.push(PlanLink {
        id: binding.task_id.clone(),
        parent_step_id: binding.parent_step_id.clone(),
        child_session_id: CHILD.into(),
        terminal: false,
        required: true,
        pending_source_revision: None,
        review_required_revision: 20,
        result_id: None,
        dispatch_failure: None,
    });
    plan.validate().unwrap();
    let delegation = PlanDelegation {
        parent_session_id: binding.parent_session_id.clone(),
        parent_scope_id: plan.scope_id.clone(),
        parent_step_id: binding.parent_step_id.clone(),
        task_id: binding.task_id.clone(),
        child_session_id: CHILD.into(),
    };
    (
        binding,
        PlanningReadSnapshot {
            timeline_seq: 123,
            plan,
            delegation: Some(delegation),
        },
    )
}

fn render_parent(
    binding: &ParentPlanBinding,
    observation: &Result<Option<PlanningReadSnapshot>, ParentPlanningReadError>,
) -> (String, Value) {
    let context = ParentPlanContext::from_observation(binding, CHILD, observation);
    let text = render_planning_context(CHILD, None, Some(context), Some(READ_ALIAS))
        .unwrap()
        .unwrap();
    let value = decode(&text);
    (text, value)
}

fn decode(text: &str) -> Value {
    assert!(text.len() <= MAX_CONTEXT_BYTES, "{} bytes", text.len());
    assert!(text.starts_with(GUIDANCE));
    serde_json::from_str(text.strip_prefix(GUIDANCE).unwrap()).unwrap()
}

fn change(snapshot: &PlanningReadSnapshot, revision: u64, stop: bool) -> ReplanChange {
    let link = &snapshot.plan.links[0];
    let step = snapshot.plan.steps.last().unwrap();
    ReplanChange {
        id: format!("{}:{}:{revision}", link.id, step.id),
        link_id: link.id.clone(),
        child_session_id: link.child_session_id.clone(),
        step_id: step.id.clone(),
        source_revision: revision,
        content: step.content.clone(),
        requirements: step.requirements.clone(),
        priority: step.priority,
        status: if stop {
            PlanStepStatus::Cancelled
        } else {
            PlanStepStatus::InProgress
        },
        removed: stop,
        progress: ReplanProgress::Pending,
    }
}

fn own_fixture(result: Option<String>) -> PlanningReadSnapshot {
    let mut plan = LocalPlan::new(CHILD, "work:self");
    plan.revision = 7;
    plan.steps.push(step(0));
    plan.links.push(PlanLink {
        id: "own-required-task".into(),
        parent_step_id: "step-0".into(),
        child_session_id: "grandchild-C".into(),
        terminal: result.is_some(),
        required: true,
        pending_source_revision: None,
        review_required_revision: 7,
        result_id: result,
        dispatch_failure: None,
    });
    plan.validate().unwrap();
    PlanningReadSnapshot {
        timeline_seq: 44,
        plan,
        delegation: None,
    }
}

#[test]
fn no_plan_and_no_binding_produces_no_context() {
    assert_eq!(
        render_planning_context(CHILD, None, None, Some(READ_ALIAS)).unwrap(),
        None
    );
}

#[test]
fn child_without_own_plan_keeps_associated_step_after_first_twelve() {
    let (binding, snapshot) = parent_fixture(17);
    let original = snapshot.clone();
    let observation = Ok(Some(snapshot));
    let (text, value) = render_parent(&binding, &observation);
    let parent = &value["parent"];
    assert_eq!(value["child_session_id"], CHILD);
    assert!(value["self"].is_null());
    assert_eq!(parent["binding"], binding_json(&binding));
    assert_eq!(parent["source"]["scope_id"], "scope:direct-parent");
    assert_eq!(parent["source"]["timeline_seq"], 123);
    assert_eq!(parent["source"]["revision"], 20);
    assert_eq!(parent["associated_step"]["id"], "step-16");
    assert_eq!(parent["associated_step"]["position"], 17);
    assert_eq!(parent["counts"]["steps"], 17);
    assert_eq!(
        parent["associated_step"]["content_summary"]["text"],
        "The current delegated objective"
    );
    assert_eq!(
        parent["associated_step"]["requirements_summary"]["items"][0]["text"],
        "Use the actual direct parent"
    );
    assert!(!text.contains("private sibling body"));
    assert_eq!(
        observation.unwrap().unwrap(),
        original,
        "rendering never mutates facts"
    );
}

#[test]
fn unavailable_parent_retains_each_actual_error_and_original_binding() {
    let (binding, _) = parent_fixture(1);
    for error in [
        ParentPlanningReadError::NoParent,
        ParentPlanningReadError::Unavailable,
        ParentPlanningReadError::TimedOut,
        ParentPlanningReadError::InvalidEvidence,
    ] {
        let (_, value) = render_parent(&binding, &Err(error));
        assert_eq!(value["parent"]["binding"], binding_json(&binding));
        assert_eq!(value["parent"]["error_code"], error.code());
        assert_eq!(value["parent"]["observation"], "unavailable");
        assert!(value["parent"]["source"].is_null());
    }
}

#[test]
fn mismatched_or_missing_evidence_cannot_replace_birth_identity() {
    let (binding, original) = parent_fixture(17);
    for case in 0..10 {
        let mut snapshot = original.clone();
        match case {
            0 => snapshot.delegation.as_mut().unwrap().parent_session_id = "root-decoy".into(),
            1 => snapshot.delegation.as_mut().unwrap().task_id = "other-execution".into(),
            2 => snapshot.delegation.as_mut().unwrap().parent_step_id = "step-0".into(),
            3 => snapshot.delegation.as_mut().unwrap().child_session_id = "sibling-decoy".into(),
            4 => snapshot.delegation = None,
            5 => snapshot.plan.session_id = "root-decoy".into(),
            6 => snapshot.plan.scope_id = "old-scope-decoy".into(),
            7 => snapshot.plan.links[0].child_session_id = "sibling-decoy".into(),
            8 => snapshot.plan.links[0].parent_step_id = "step-0".into(),
            9 => snapshot.plan.steps.pop().map(|_| ()).unwrap(),
            _ => unreachable!(),
        }
        let (text, value) = render_parent(&binding, &Ok(Some(snapshot)));
        assert_eq!(
            value["parent"]["error_code"], "invalid_evidence",
            "case {case}"
        );
        assert_eq!(value["parent"]["binding"], binding_json(&binding));
        assert!(
            !text.contains("decoy"),
            "rejected evidence must not become context"
        );
    }
    let (_, value) = render_parent(&binding, &Ok(None));
    assert_eq!(value["parent"]["error_code"], "invalid_evidence");
}

#[test]
fn final_serialized_budget_includes_header_json_and_escape_cost() {
    let content = "x".repeat(100_000);
    let mut context = Context(json!({"field": {"text": "", "omitted_bytes": content.len()}}));
    context.text("/field", &content, content.len());
    let before = context.0.clone();
    assert_eq!(GUIDANCE.len() + before.to_string().len(), MAX_CONTEXT_BYTES);
    assert!(!context.update(|value| value["extra"] = json!("x")));
    assert_eq!(context.0, before);
    assert_eq!(context.finish().len(), MAX_CONTEXT_BYTES);

    for content in ["汉字😀".repeat(4096), "\u{0000}\n\r\t\\\"".repeat(4096)] {
        let mut context = Context(json!({"field": {"text": "", "omitted_bytes": content.len()}}));
        context.text("/field", &content, content.len());
        let text = context.finish();
        let value = decode(&text);
        let shown = value["field"]["text"].as_str().unwrap();
        assert!(content.starts_with(shown));
        assert!(content.is_char_boundary(shown.len()));
        assert_eq!(value["field"]["omitted_bytes"], content.len() - shown.len());
        assert!(
            text.len() > MAX_CONTEXT_BYTES - 12,
            "budget should be used at a UTF-8 boundary"
        );
    }
}

#[test]
fn eleven_multibyte_steps_cannot_exceed_the_total_budget() {
    let mut snapshot = own_fixture(None);
    snapshot.plan.links.clear();
    snapshot.plan.steps = (0..11)
        .map(|index| {
            let mut step = step(index);
            step.content = "汉".repeat(256);
            step
        })
        .collect();
    snapshot.plan.validate().unwrap();
    let text = render_planning_context(CHILD, Some(&snapshot), None, Some(READ_ALIAS))
        .unwrap()
        .unwrap();
    let value = decode(&text);
    let shown = value["self"]["steps_summary"].as_array().unwrap().len();
    assert_eq!(value["self"]["counts"]["steps"], 11);
    assert_eq!(value["self"]["omitted_counts"]["steps"], 11 - shown);
}

#[test]
fn max_text_fields_and_4096_steps_keep_last_tombstone_identity_and_stop() {
    let (binding, mut snapshot) = parent_fixture(4096);
    let huge = "汉😀\n\\\"".repeat(8192);
    let mut end = (64 * 1024).min(huge.len());
    while !huge.is_char_boundary(end) {
        end -= 1;
    }
    let mut huge = huge[..end].to_owned();
    huge.push_str(&"x".repeat(64 * 1024 - huge.len()));
    assert_eq!(huge.len(), 64 * 1024);
    let last = snapshot.plan.steps.last_mut().unwrap();
    last.content = huge.clone();
    last.requirements = Some(vec![huge.clone(), huge.clone(), huge.clone()]);
    last.removed = true;
    last.status = PlanStepStatus::Cancelled;
    snapshot.plan.replans = (1..=17)
        .map(|revision| change(&snapshot, revision, revision == 17))
        .collect();
    snapshot.plan.links[0].pending_source_revision = Some(17);
    snapshot.plan.validate().unwrap();
    let (text, value) = render_parent(&binding, &Ok(Some(snapshot.clone())));
    let parent = &value["parent"];
    assert_eq!(parent["associated_step"]["position"], 4096);
    assert_eq!(parent["counts"]["steps"], 4096);
    assert_eq!(parent["associated_step"]["removed"], true);
    assert_eq!(parent["associated_step"]["id"], binding.parent_step_id);
    assert_eq!(parent["control_summary"]["directive"], "stop_and_wrap_up");
    assert_eq!(parent["control_summary"]["unhandled_count"], 17);
    assert_eq!(
        parent["control_summary"]["records"][0]["source_revision"],
        17
    );
    assert_eq!(
        parent["control_summary"]["records"][0]["directive"],
        "stop_and_wrap_up"
    );
    assert_eq!(
        parent["associated_step"]["requirements_summary"]["count"],
        3
    );
    assert!(
        parent["associated_step"]["content_summary"]["omitted_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(text.len() <= MAX_CONTEXT_BYTES);
    assert_eq!(snapshot.plan.steps.last().unwrap().content, huge);
}

#[test]
fn parent_replan_projection_is_background_until_the_notification_arrives() {
    let (binding, mut snapshot) = parent_fixture(2);
    snapshot.plan.replans.push(change(&snapshot, 20, false));
    snapshot.plan.links[0].pending_source_revision = Some(20);
    snapshot.plan.validate().unwrap();
    let (text, value) = render_parent(&binding, &Ok(Some(snapshot)));
    assert!(text.contains("without a routine check or confirmation"));
    assert!(text.contains("Use newly delivered parent-change notices"));
    assert_eq!(value["parent"]["control_summary"]["directive"], "none");
    assert_eq!(value["parent"]["control_summary"]["unhandled_count"], 1);
    assert_eq!(
        value["parent"]["control_summary"]["records"][0]["progress"],
        "pending"
    );
}

#[test]
fn historical_stop_is_visible_after_the_step_reopens() {
    for progress in [
        ReplanProgress::Received,
        ReplanProgress::Handled {
            outcome: ReplanOutcome::Stopping,
        },
        ReplanProgress::Unapplied {
            reason: "execution was already terminal".into(),
        },
    ] {
        let (binding, mut snapshot) = parent_fixture(17);
        let mut stop = change(&snapshot, 19, true);
        stop.progress = progress;
        snapshot.plan.links[0].pending_source_revision = is_unhandled(&stop).then_some(19);
        snapshot.plan.replans.push(stop);
        snapshot.plan.validate().unwrap();
        let (_, value) = render_parent(&binding, &Ok(Some(snapshot)));
        assert_eq!(value["parent"]["associated_step"]["status"], "in_progress");
        assert_eq!(value["parent"]["control_summary"]["stop_record_count"], 1);
        assert_eq!(
            value["parent"]["control_summary"]["directive"],
            "stop_and_wrap_up"
        );
    }
}

#[test]
fn long_optional_review_reference_is_omitted_as_a_whole_without_fake_result() {
    let mut result = "reference\\\"".repeat(6000);
    result.truncate(64 * 1024);
    assert_eq!(result.len(), 64 * 1024);
    let snapshot = own_fixture(Some(result.clone()));
    let original = snapshot.clone();
    let text = render_planning_context(CHILD, Some(&snapshot), None, Some(READ_ALIAS))
        .unwrap()
        .unwrap();
    let value = decode(&text);
    assert_eq!(value["self"]["unresolved_required_delivery_count"], 1);
    assert_eq!(value["self"]["omitted_counts"]["links"], 1);
    assert!(
        value["self"]["links_summary"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!text.contains("\"resultId\":"));
    assert!(!text.contains("reference"));
    assert_eq!(snapshot, original);
    assert_eq!(
        snapshot.plan.links[0].result_id.as_deref(),
        Some(result.as_str())
    );
}

#[test]
fn complete_review_bindings_keep_the_exact_result_or_real_null() {
    for result in [
        None,
        Some("{\"timeline_id\":\"grandchild-C\",\"first_seq\":42,\"last_seq\":42}".into()),
    ] {
        let snapshot = own_fixture(result);
        let text = render_planning_context(CHILD, Some(&snapshot), None, Some(READ_ALIAS))
            .unwrap()
            .unwrap();
        let value = decode(&text);
        assert_eq!(
            value["self"]["links_summary"][0]["review_binding"],
            review_binding(&snapshot.plan, &snapshot.plan.links[0])
        );
        assert_eq!(value["self"]["omitted_counts"]["links"], 0);
    }
}

#[test]
fn resolved_read_alias_is_exact_and_missing_read_entry_invents_no_call() {
    let snapshot = own_fixture(None);
    let alias = "host_alias_汉_\\\"read";
    let text = render_planning_context(CHILD, Some(&snapshot), None, Some(alias))
        .unwrap()
        .unwrap();
    assert_eq!(decode(&text)["read_entry"]["tool"], alias);
    let oversized_alias = "x".repeat(64 * 1024);
    for name in [None, Some(oversized_alias.as_str())] {
        let text = render_planning_context(CHILD, Some(&snapshot), None, name)
            .unwrap()
            .unwrap();
        let value = decode(&text);
        assert!(value["read_entry"]["tool"].is_null());
        assert_ne!(value["read_entry"]["availability"], "available");
        assert!(!text.contains("todo_write"));
        assert!(!text.contains("todo_read"));
    }
}

#[test]
fn maximum_escaped_valid_identities_survive_with_parent_and_self_together() {
    let (mut binding, mut parent) = parent_fixture(1);
    binding.parent_session_id = format!("{}p", "\"".repeat(127));
    binding.task_id = "\"".repeat(128);
    binding.parent_step_id = "\\".repeat(128);
    let child = format!("{}c", "\\".repeat(127));
    parent.plan.session_id = binding.parent_session_id.clone();
    parent.plan.scope_id = "\"".repeat(128);
    parent.plan.steps[0].id = binding.parent_step_id.clone();
    parent.plan.links[0].id = binding.task_id.clone();
    parent.plan.links[0].parent_step_id = binding.parent_step_id.clone();
    parent.plan.links[0].child_session_id = child.clone();
    let mut replan = change(&parent, 19, false);
    replan.child_session_id = child.clone();
    parent.plan.replans.push(replan);
    parent.plan.links[0].pending_source_revision = Some(19);
    parent.delegation = Some(PlanDelegation {
        parent_session_id: binding.parent_session_id.clone(),
        parent_scope_id: parent.plan.scope_id.clone(),
        parent_step_id: binding.parent_step_id.clone(),
        task_id: binding.task_id.clone(),
        child_session_id: child.clone(),
    });
    parent.plan.validate().unwrap();
    let mut own = own_fixture(None);
    own.plan.session_id = child.clone();
    own.plan.scope_id = "\\".repeat(128);
    own.plan.validate().unwrap();
    let observation = Ok(Some(parent));
    let context = ParentPlanContext::from_observation(&binding, &child, &observation);
    let text = render_planning_context(&child, Some(&own), Some(context), Some(READ_ALIAS))
        .unwrap()
        .unwrap();
    let value = decode(&text);
    assert_eq!(value["child_session_id"], child);
    assert_eq!(value["parent"]["binding"], binding_json(&binding));
    assert_eq!(
        value["parent"]["associated_step"]["id"],
        binding.parent_step_id
    );
    assert_eq!(value["parent"]["control_summary"]["unhandled_count"], 1);
    assert_eq!(value["self"]["unresolved_required_delivery_count"], 1);
}

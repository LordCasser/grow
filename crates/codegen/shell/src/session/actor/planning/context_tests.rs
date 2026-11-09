use super::*;
use tool_types::planning::{PlanLink, PlanPriority};

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

fn decode(text: &str) -> Value {
    assert!(text.len() <= MAX_CONTEXT_BYTES, "{} bytes", text.len());
    assert!(text.starts_with(GUIDANCE));
    serde_json::from_str(text.strip_prefix(GUIDANCE).unwrap()).unwrap()
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
    let text = render_planning_context(&snapshot, Some(READ_ALIAS)).unwrap();
    let value = decode(&text);
    let shown = value["self"]["steps_summary"].as_array().unwrap().len();
    assert_eq!(value["self"]["counts"]["steps"], 11);
    assert_eq!(value["self"]["omitted_counts"]["steps"], 11 - shown);
}

#[test]
fn long_optional_review_reference_is_omitted_as_a_whole_without_fake_result() {
    let mut result = "reference\\\"".repeat(6000);
    result.truncate(64 * 1024);
    assert_eq!(result.len(), 64 * 1024);
    let snapshot = own_fixture(Some(result.clone()));
    let original = snapshot.clone();
    let text = render_planning_context(&snapshot, Some(READ_ALIAS)).unwrap();
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
        let text = render_planning_context(&snapshot, Some(READ_ALIAS)).unwrap();
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
    let text = render_planning_context(&snapshot, Some(alias)).unwrap();
    assert_eq!(decode(&text)["read_entry"]["tool"], alias);
    let oversized_alias = "x".repeat(64 * 1024);
    for name in [None, Some(oversized_alias.as_str())] {
        let text = render_planning_context(&snapshot, name).unwrap();
        let value = decode(&text);
        assert!(value["read_entry"]["tool"].is_null());
        assert_ne!(value["read_entry"]["availability"], "available");
        assert!(!text.contains("todo_write"));
        assert!(!text.contains("todo_read"));
    }
}

use super::*;
use tool_types::{PlanPriority, PlanStepStatus, ReplanOutcome, ReplanProgress};

fn change() -> ReplanChange {
    ReplanChange {
        id: "change-1".into(),
        link_id: "task-1".into(),
        child_session_id: "child-1".into(),
        step_id: "step-1".into(),
        source_revision: 9,
        content: "Preserve the completed investigation \"界🙂\"".into(),
        requirements: Some(vec!["Check the revised behavior".into()]),
        priority: PlanPriority::High,
        status: PlanStepStatus::InProgress,
        removed: false,
        progress: ReplanProgress::Dispatching,
    }
}

fn details(body: &str) -> Value {
    assert!(body.len() <= MAX_GUIDANCE_BYTES);
    serde_json::from_str(body.split_once('\n').unwrap().1).unwrap()
}

#[test]
fn fixed_prefix_uses_the_shared_stop_classification() {
    let mut change = change();
    let body = guidance_body("parent", &change).unwrap();
    assert_eq!(body.split_once('\n').unwrap().0, REPLAN_PREFIX);
    let value = details(&body);
    assert_eq!(value["directive"], "replan");
    assert_eq!(value["parent_session_id"], "parent");
    assert_eq!(value["child_session_id"], change.child_session_id);
    assert_eq!(value["link_id"], change.link_id);
    assert_eq!(value["step_id"], change.step_id);
    assert_eq!(value["change_id"], change.id);
    assert_eq!(value["source_revision"], change.source_revision);
    assert_eq!(value["content_summary"]["text"], change.content);
    assert_eq!(value["content_summary"]["omitted_bytes"], 0);
    assert_eq!(
        value["requirements_summary"]["items"][0]["omitted_bytes"],
        0
    );
    assert_eq!(value["requirements_summary"]["omitted_items"], 0);
    for (removed, status) in [
        (true, PlanStepStatus::InProgress),
        (false, PlanStepStatus::Cancelled),
    ] {
        change.removed = removed;
        change.status = status;
        let body = guidance_body("parent", &change).unwrap();
        assert_eq!(body.split_once('\n').unwrap().0, STOP_PREFIX);
        assert_eq!(details(&body)["directive"], "stop");
    }
}

#[test]
fn maximum_legal_payload_and_escaped_identities_fit_the_final_utf8_budget() {
    let mut change = change();
    change.id = "\"\\".repeat(139);
    change.link_id = "\"\\".repeat(64);
    change.child_session_id = "\"\\".repeat(64);
    change.step_id = "\"\\".repeat(64);
    change.source_revision = u64::MAX;
    let parent = "\"\\".repeat(64);
    for sample in ["x", "界🙂", "\"\\\u{00}\n界🙂"] {
        change.content = sample.repeat((64 * 1024) / sample.len());
        change.requirements = Some(vec![change.content.clone(); 256]);
        let mut plan = tool_types::LocalPlan::new(change.child_session_id.clone(), "scope");
        let mut received = change.clone();
        received.progress = ReplanProgress::Received;
        plan.receive_replan(received).unwrap();
        plan.validate().unwrap();
        for removed in [false, true] {
            change.removed = removed;
            let body = guidance_body(&parent, &change).unwrap();
            let value = details(&body);
            assert_eq!(value["parent_session_id"], parent);
            assert_eq!(value["child_session_id"], change.child_session_id);
            assert_eq!(value["link_id"], change.link_id);
            assert_eq!(value["step_id"], change.step_id);
            assert_eq!(value["change_id"], change.id);
            let content = &value["content_summary"];
            let text = content["text"].as_str().unwrap();
            assert!(change.content.starts_with(text));
            assert_eq!(
                content["omitted_bytes"].as_u64().unwrap() as usize,
                change.content.len() - text.len()
            );
            assert!(content["omitted_bytes"].as_u64().unwrap() > 0);
            let requirements = &value["requirements_summary"];
            assert_eq!(requirements["total"], 256);
            assert_eq!(requirements["items"].as_array().unwrap().len(), 4);
            assert_eq!(requirements["omitted_items"], 252);
            for summary in requirements["items"].as_array().unwrap() {
                let text = summary["text"].as_str().unwrap();
                assert!(change.content.starts_with(text));
                assert_eq!(
                    summary["omitted_bytes"].as_u64().unwrap() as usize,
                    change.content.len() - text.len()
                );
            }
        }
    }
}

#[test]
fn guidance_is_byte_stable_across_delivery_progress() {
    let mut change = change();
    let expected = guidance_body("parent", &change).unwrap();
    for progress in [
        ReplanProgress::Received,
        ReplanProgress::Consumed,
        ReplanProgress::Handled {
            outcome: ReplanOutcome::Updated { plan_revision: 99 },
        },
    ] {
        change.progress = progress;
        assert_eq!(guidance_body("parent", &change).unwrap(), expected);
    }
    let value = details(&expected);
    assert!(value.get("progress").is_none());
    assert!(value.get("plan_revision").is_none());
}

#[test]
fn summaries_count_actual_json_escapes_and_preserve_unicode_boundaries() {
    let text = "\u{00}\u{08}\u{0b}\u{0c}\n\r\t\"\\界🙂";
    for budget in 0..100 {
        let summary = text_summary(text, budget);
        let excerpt = summary["text"].as_str().unwrap();
        assert!(text.starts_with(excerpt));
        assert!(serde_json::to_string(excerpt).unwrap().len() - 2 <= budget);
        assert_eq!(summary["omitted_bytes"], text.len() - excerpt.len());
    }
    let mut change = change();
    change.content.clear();
    change.requirements = None;
    let value = details(&guidance_body("parent", &change).unwrap());
    assert_eq!(value["content_summary"]["text"], "");
    assert_eq!(value["content_summary"]["omitted_bytes"], 0);
    assert_eq!(value["requirements_summary"]["total"], 0);
    assert!(
        value["requirements_summary"]["items"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

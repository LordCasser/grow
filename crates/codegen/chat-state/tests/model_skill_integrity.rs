use chat_state::compaction_utils::repair_history;
use sampling_types::*;

fn completed_batch() -> Vec<ConversationItem> {
    let skill = SkillLoadItem {
        input_id: "model:a".into(),
        reference_index: 0,
        call_id: "a".into(),
        skill_name: "audit".into(),
        source: SkillLoadSource::Model,
        args: "".into(),
        prepared_body: "audit body".into(),
        prompt_index: Some(1),
    };
    vec![
        ConversationItem::assistant_tool_calls(vec![
            ToolCall {
                id: "a".into(),
                name: "load_skill".into(),
                arguments: r#"{"name":"audit","args":""}"#.into(),
            },
            ToolCall {
                id: "b".into(),
                name: "write_file".into(),
                arguments: r#"{"path":"audit.txt"}"#.into(),
            },
        ]),
        ConversationItem::tool_result("a", skill_load_result_content(&skill)),
        ConversationItem::SkillLoad(skill),
        ConversationItem::tool_result("b", "write completed successfully"),
    ]
}
#[test]
fn history_repair_keeps_completed_sibling_across_model_skill_metadata() {
    let mut items = completed_batch();
    let report = repair_history(&mut items);
    assert!(report.stripped_tool_result_ids.is_empty());
    assert_eq!(report.synthetic_results_inserted, 0);
    let output = items
        .iter()
        .find_map(|item| match item {
            ConversationItem::ToolResult(result) if result.tool_call_id == "b" => {
                Some(result.content.as_ref())
            }
            _ => None,
        })
        .unwrap();
    assert_eq!(output, "write completed successfully");
    assert!(!repair_history(&mut items).changed());
}

#[test]
fn missing_result_is_repaired_without_replacing_completed_results() {
    let mut items = completed_batch();
    items.pop();
    let report = repair_history(&mut items);
    assert_eq!(report.synthetic_results_inserted, 1);
    assert!(report.stripped_tool_result_ids.is_empty());
    assert!(items.iter().any(|v| matches!(v, ConversationItem::ToolResult(r) if r.tool_call_id == "a" && r.content.contains("audit body"))));
    assert!(!repair_history(&mut items).changed());
}

#[test]
fn duplicate_results_across_metadata_keep_the_last_real_result() {
    let mut items = completed_batch();
    items.insert(1, ConversationItem::tool_result("b", "stale duplicate"));
    let report = repair_history(&mut items);
    assert_eq!(report.duplicates_removed, 1);
    assert_eq!(report.synthetic_results_inserted, 0);
    let results: Vec<_> = items
        .iter()
        .filter_map(|v| match v {
            ConversationItem::ToolResult(r) if r.tool_call_id == "b" => Some(r.content.as_ref()),
            _ => None,
        })
        .collect();
    assert_eq!(results, ["write completed successfully"]);
}

#[test]
fn metadata_normalization_does_not_cross_real_or_unbound_boundaries() {
    for boundary in [
        ConversationItem::user("new input"),
        {
            let mut fact = completed_batch().remove(2);
            if let ConversationItem::SkillLoad(s) = &mut fact {
                s.source = SkillLoadSource::User;
            }
            fact
        },
        {
            let mut fact = completed_batch().remove(2);
            if let ConversationItem::SkillLoad(s) = &mut fact {
                s.call_id = "unrelated".into();
            }
            fact
        },
    ] {
        let mut items = completed_batch();
        items[2] = boundary;
        let before = serde_json::to_value(&items).unwrap();
        assert_eq!(normalize_model_skill_results(&mut items), 0);
        assert_eq!(serde_json::to_value(&items).unwrap(), before);
        let report = repair_history(&mut items);
        assert_eq!(report.stripped_tool_result_ids, ["b"]);
    }
}

#[test]
fn read_only_dangling_query_recognizes_model_skill_metadata() {
    let mut items = completed_batch();
    assert!(!has_dangling_tool_calls(&items));
    items.pop();
    assert!(has_dangling_tool_calls(&items));
}

#[test]
fn several_skill_results_preserve_batch_order_and_metadata() {
    let mut items = completed_batch();
    if let ConversationItem::Assistant(a) = &mut items[0] {
        a.tool_calls.push(ToolCall {
            id: "c".into(),
            name: "read_file".into(),
            arguments: "{}".into(),
        });
    }
    let mut second = items[2].clone();
    if let ConversationItem::SkillLoad(s) = &mut second {
        s.call_id = "b".into();
    }
    items.push(second);
    items.push(ConversationItem::tool_result("c", "third actual result"));
    let report = repair_history(&mut items);
    assert_eq!(report.normalized_tool_batches, 1);
    assert_eq!(report.synthetic_results_inserted, 0);
    assert!(report.stripped_tool_result_ids.is_empty());
    let ids: Vec<_> = items[1..4]
        .iter()
        .map(|v| match v {
            ConversationItem::ToolResult(r) => r.tool_call_id.as_str(),
            _ => panic!("split results"),
        })
        .collect();
    assert_eq!(ids, ["a", "b", "c"]);
    assert!(matches!(&items[4], ConversationItem::SkillLoad(s) if s.call_id == "a"));
    assert!(matches!(&items[5], ConversationItem::SkillLoad(s) if s.call_id == "b"));
    assert!(!repair_history(&mut items).changed());
}

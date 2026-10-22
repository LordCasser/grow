//! Audit-only reproduction of model SkillLoad crossing a tool-result run.
use chat_state::compaction_utils::{repair_history, strip_displaced_tool_results};
use sampling_types::*;

fn completed_batch() -> Vec<ConversationItem> {
    let skill = SkillLoadItem {
        input_id: "model:a".into(), reference_index: 0, call_id: "a".into(),
        skill_name: "audit".into(), source: SkillLoadSource::Model,
        args: "".into(), prepared_body: "audit body".into(), prompt_index: Some(1),
    };
    vec![
        ConversationItem::assistant_tool_calls(vec![
            ToolCall { id: "a".into(), name: "load_skill".into(), arguments: r#"{"name":"audit","args":""}"#.into() },
            ToolCall { id: "b".into(), name: "write_file".into(), arguments: r#"{"path":"audit.txt"}"#.into() },
        ]),
        ConversationItem::tool_result("a", skill_load_result_content(&skill)),
        ConversationItem::SkillLoad(skill),
        ConversationItem::tool_result("b", "write completed successfully"),
    ]
}
#[test]
fn history_repair_discards_completed_sibling_and_reports_it_unexecuted() {
    let mut items = completed_batch();
    let report = repair_history(&mut items);
    assert_eq!(report.stripped_tool_result_ids, ["b"]);
    assert_eq!(report.synthetic_results_inserted, 1);
    let output = items.iter().find_map(|item| match item {
        ConversationItem::ToolResult(result) if result.tool_call_id == "b" => Some(result.content.as_ref()),
        _ => None,
    }).unwrap();
    assert!(output.contains("not executed"));
    assert!(!output.contains("completed successfully"));
}
#[test]
fn request_boundary_repair_reason_can_falsely_attribute_user_cancellation() {
    let mut items = completed_batch();
    // Same pure transforms and reason used by the production request boundary.
    assert_eq!(dedup_duplicate_tool_results(&mut items), 0);
    assert_eq!(strip_displaced_tool_results(&mut items), ["b"]);
    assert_eq!(repair_dangling_tool_calls(&mut items, DanglingToolCallReason::UserCancelled), 1);
    let output = items.iter().find_map(|item| match item {
        ConversationItem::ToolResult(result) if result.tool_call_id == "b" => Some(result.content.as_ref()),
        _ => None,
    }).unwrap();
    assert_eq!(output, "Tool execution was cancelled by the user (tool `write_file` was not executed).");
}

use super::FrozenQuestion;
use chat_state::{EventSeq, SurfaceId, TimelineMaterialization, TimelineRangeRef};
use sampling_types::{
    ConversationItem, ConversationRequest, SkillLoadItem, SkillLoadSource, ToolCall,
    project_portable_history,
};

#[test]
fn question_budget_drops_tool_batch_with_its_skill_metadata() {
    let call = |id: &str| ToolCall {
        id: id.into(),
        name: "load_skill".into(),
        arguments: r#"{"name":"reader"}"#.into(),
    };
    let skill = |id: &str| {
        ConversationItem::SkillLoad(SkillLoadItem {
            input_id: id.into(),
            reference_index: 0,
            call_id: id.into(),
            skill_name: "reader".into(),
            source: SkillLoadSource::Model,
            args: "{}".into(),
            prepared_body: "Skill body".into(),
            prompt_index: None,
        })
    };
    let surface = vec![
        ConversationItem::system("First system instruction"),
        ConversationItem::assistant_tool_calls(vec![
            call("duplicate"),
            call("valid"),
            call("pending"),
        ]),
        ConversationItem::tool_result("duplicate", "Repeated call identifier"),
        skill("duplicate"),
        ConversationItem::tool_result("valid", "Retained tool evidence"),
        skill("valid"),
        ConversationItem::system("Second system instruction"),
        ConversationItem::assistant("Recent independent evidence"),
        ConversationItem::assistant_tool_calls(vec![call("duplicate")]),
    ];
    let surface_ids = (0..surface.len())
        .map(|item| SurfaceId {
            event: EventSeq::new(1),
            item: item.try_into().unwrap(),
        })
        .collect();
    let snapshot = TimelineMaterialization {
        input_ref: TimelineRangeRef {
            timeline_id: "test".into(),
            first_seq: 1,
            last_seq: 1,
        },
        surface_revision: 2,
        surface,
        surface_ids,
        unconsumed_skill_load_ids: Vec::new(),
        active_control_contexts: Default::default(),
    };
    let prompt = "What is the recent evidence?";
    let question = ConversationItem::user(prompt);
    let mut portable = project_portable_history(&snapshot.surface);
    portable.push(question.clone());
    let full = chat_state::estimate_request_input_tokens(&ConversationRequest {
        items: portable.clone(),
        ..Default::default()
    });
    let selected =
        FrozenQuestion::select(&snapshot, prompt.into(), "test-model".into(), full).unwrap();
    let ids = &snapshot.surface_ids;
    assert_eq!(
        selected.context_ids,
        [ids[0], ids[1], ids[4], ids[3], ids[5], ids[6], ids[7]]
    );
    assert_eq!(
        serde_json::to_value(selected.request.items).unwrap(),
        serde_json::to_value(portable).unwrap()
    );

    // The two skill facts could fit on their own. The batch nevertheless must
    // leave together once the assistant and its result no longer fit.
    let limit = [0, 3, 5, 6, 7]
        .into_iter()
        .map(|index| chat_state::estimate_item_tokens(&snapshot.surface[index]))
        .sum::<u64>()
        + chat_state::estimate_item_tokens(&question);
    assert!(limit < full);
    let selected =
        FrozenQuestion::select(&snapshot, prompt.into(), "test-model".into(), limit).unwrap();
    assert_eq!(selected.context_ids, [ids[0], ids[6], ids[7]]);
    assert_eq!(
        serde_json::to_value(selected.request.items).unwrap(),
        serde_json::to_value([
            snapshot.surface[0].clone(),
            snapshot.surface[6].clone(),
            snapshot.surface[7].clone(),
            question,
        ])
        .unwrap()
    );
}

use chat_state::{
    CompactionEvent, Timeline, TimelineEvent, TimelineEventKind, estimate_conversation_tokens,
};
use sampling_types::{ConversationItem, project_conversation_for_goal_scope};
use std::collections::BTreeSet;
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let events = std::fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<TimelineEvent>(line).unwrap())
        .collect::<Vec<_>>();
    for (index, event) in events.iter().enumerate() {
        if !matches!(
            &event.kind,
            TimelineEventKind::Compaction(CompactionEvent::Started { .. })
        ) {
            continue;
        }
        let timeline = Timeline::from_events(events[..index].to_vec()).unwrap();
        let active = if index == 2461 {
            timeline.surface().iter().find_map(|item| match item {
                ConversationItem::User(u) => u.goal_directive.as_ref(),
                _ => None,
            })
        } else {
            None
        };
        let surface = project_conversation_for_goal_scope(timeline.surface().to_vec(), active);
        println!(
            "before_event={} surface={} ids={} estimated_tokens={}",
            event.seq.get(),
            surface.len(),
            timeline.surface_ids().len(),
            estimate_conversation_tokens(&surface)
        );
        for (index, item) in surface.iter().enumerate() {
            if let ConversationItem::User(user) = item {
                if user.prompt_index.is_some() {
                    println!(
                        "prompt index={index} coordinate={:?} suffix_tokens={}",
                        user.prompt_index,
                        estimate_conversation_tokens(&surface[index..])
                    );
                }
            }
        }
        for retain in [40960, 32000, 16000] {
            let plan = chat_state::compaction_utils::plan_compaction_range(
                &surface,
                timeline.surface_ids(),
                retain,
                5000,
            );
            println!(
                "retain={retain} plan={:?}",
                plan.as_ref()
                    .map(|p| (p.start_index, p.end_index, p.source_tokens))
            );
            if let Some(p) = plan {
                let tail = &surface[p.end_index + 1..];
                assert!(estimate_conversation_tokens(tail) >= retain);
                for slice in [&surface[p.start_index..=p.end_index], tail] {
                    let calls = slice
                        .iter()
                        .flat_map(|item| match item {
                            ConversationItem::Assistant(a) => {
                                a.tool_calls.iter().map(|c| c.id.to_string()).collect()
                            }
                            _ => Vec::new(),
                        })
                        .collect::<BTreeSet<_>>();
                    let results = slice
                        .iter()
                        .filter_map(|item| match item {
                            ConversationItem::ToolResult(r) => Some(r.tool_call_id.clone()),
                            _ => None,
                        })
                        .collect::<BTreeSet<_>>();
                    assert_eq!(calls, results);
                }
                println!(
                    "tail_tokens={} tool_pairs=complete source_start={:?} source_end={:?}",
                    estimate_conversation_tokens(tail),
                    p.target.start,
                    p.target.end
                );
            }
        }
    }
}

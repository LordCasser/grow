// Complete temporary probe, removed from the test build after measurement.
// Paste into actor::request_builder::tests, which provides the normal imports.

fn healthy_request_fixture(kind: &str, item_count: usize) -> Vec<ConversationItem> {
    match kind {
        "text" => (0..item_count)
            .map(|index| ConversationItem::user(format!("ordinary input {index}")))
            .collect(),
        "tool" => {
            let mut items = Vec::with_capacity(item_count);
            for index in 0..item_count / 2 {
                let id = format!("call-{index}");
                items.push(ConversationItem::assistant_tool_calls(vec![
                    sampling_types::ToolCall {
                        id: id.clone().into(),
                        name: "read_file".into(),
                        arguments: "{\"path\":\"src/lib.rs\"}".into(),
                    },
                ]));
                items.push(ConversationItem::tool_result(&id, "small tool result"));
            }
            if item_count % 2 == 1 {
                items.push(ConversationItem::user("final input"));
            }
            items
        }
        "image" => (0..item_count)
            .map(|index| {
                let mut item = ConversationItem::user(format!("image input {index}"));
                item.add_image(format!("data:image/png;base64,{index}"));
                item
            })
            .collect(),
        _ => unreachable!("known probe fixture"),
    }
}

fn old_healthy_request_copy(items: &[ConversationItem]) -> Vec<ConversationItem> {
    let mut integrity_copy = items.to_vec();
    let report = crate::compaction_utils::repair_history_with_reason(
        &mut integrity_copy,
        sampling_types::DanglingToolCallReason::UserCancelled,
    );
    assert!(!report.changed());
    std::hint::black_box(integrity_copy.len());
    drop(integrity_copy);
    items.to_vec()
}

fn reused_healthy_request_copy(items: &[ConversationItem]) -> Vec<ConversationItem> {
    let mut reusable_copy = items.to_vec();
    let report = crate::compaction_utils::repair_history_with_reason(
        &mut reusable_copy,
        sampling_types::DanglingToolCallReason::UserCancelled,
    );
    assert!(!report.changed());
    reusable_copy
}

#[test]
#[ignore = "temporary C4 health-path timing probe"]
fn healthy_request_integrity_clone_cost_probe() {
    use std::time::Instant;

    const WARMUPS: usize = 2;
    const SAMPLES: usize = 7;
    const REPETITIONS: usize = 16;
    let mut raw = Vec::new();
    for kind in ["text", "tool", "image"] {
        for item_count in [128, 512, 2_048, 8_192] {
            let fixture = healthy_request_fixture(kind, item_count);
            let old = old_healthy_request_copy(&fixture);
            let new = reused_healthy_request_copy(&fixture);
            assert_eq!(
                serialized_items(&old),
                serialized_items(&new),
                "old and new healthy request items must be identical ({kind}, {item_count})"
            );
            for _ in 0..WARMUPS {
                std::hint::black_box(old_healthy_request_copy(&fixture));
                std::hint::black_box(reused_healthy_request_copy(&fixture));
            }
            let mut old_samples = Vec::with_capacity(SAMPLES);
            let mut new_samples = Vec::with_capacity(SAMPLES);
            for _ in 0..SAMPLES {
                let start = Instant::now();
                for _ in 0..REPETITIONS {
                    std::hint::black_box(old_healthy_request_copy(&fixture));
                }
                old_samples.push(start.elapsed().as_nanos() / REPETITIONS as u128);

                let start = Instant::now();
                for _ in 0..REPETITIONS {
                    std::hint::black_box(reused_healthy_request_copy(&fixture));
                }
                new_samples.push(start.elapsed().as_nanos() / REPETITIONS as u128);
            }
            let mut sorted_old = old_samples.clone();
            let mut sorted_new = new_samples.clone();
            sorted_old.sort_unstable();
            sorted_new.sort_unstable();
            let record = format!(
                "kind={kind} items={item_count} reps={REPETITIONS} old_ns={old_samples:?} old_median_ns={} new_ns={new_samples:?} new_median_ns={}",
                sorted_old[SAMPLES / 2],
                sorted_new[SAMPLES / 2]
            );
            println!("{record}");
            raw.push(record);
        }
    }
    std::fs::write(
        "/tmp/grow-request-integrity-probe-raw.txt",
        raw.join("\n") + "\n",
    )
    .expect("write C4 raw timing samples");
}

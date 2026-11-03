// Temporary ignored-test fixture and kernel probe source, copied from compaction_utils.rs tests.
// It uses the test-only old_quarantine_malformed_tool_exchanges reference above it.

    fn malformed_bench_fixture(n: usize, k: usize) -> Vec<ConversationItem> {
        let mut items = Vec::with_capacity(n);
        for slot in 0..k {
            let target = slot * n / k;
            while items.len() < target {
                items.push(ConversationItem::user("filler"));
            }
            items.push(ConversationItem::assistant_tool_calls(vec![
                sampling_types::ToolCall {
                    id: "".into(),
                    name: "malformed".into(),
                    arguments: "{}".into(),
                },
            ]));
            items.push(ConversationItem::tool_result("", "evidence"));
        }
        while items.len() < n {
            items.push(ConversationItem::user("filler"));
        }
        items
    }

    fn run_quarantine_kernel_probe(label: &str, original: &[ConversationItem]) {
        let sample = |repair: &dyn Fn(&mut Vec<ConversationItem>) -> usize| {
            let mut items = original.to_vec();
            let started = std::time::Instant::now();
            let count = repair(&mut items);
            let elapsed = started.elapsed().as_nanos();
            let json = serde_json::to_vec(&items).unwrap();
            (elapsed, count, json)
        };
        for _ in 0..2 {
            let (_, old_count, old_json) = sample(&old_quarantine_malformed_tool_exchanges);
            let (_, new_count, new_json) = sample(&quarantine_malformed_tool_exchanges);
            assert_eq!(new_count, old_count);
            assert_eq!(new_json, old_json);
        }
        let mut old_samples = Vec::with_capacity(7);
        let mut new_samples = Vec::with_capacity(7);
        for _ in 0..7 {
            let (old_ns, old_count, old_json) = sample(&old_quarantine_malformed_tool_exchanges);
            let (new_ns, new_count, new_json) = sample(&quarantine_malformed_tool_exchanges);
            assert_eq!(new_count, old_count);
            assert_eq!(new_json, old_json);
            old_samples.push(old_ns);
            new_samples.push(new_ns);
        }
        let median = |values: &[u128]| {
            let mut sorted = values.to_vec();
            sorted.sort_unstable();
            sorted[sorted.len() / 2]
        };
        eprintln!(
            "HISTORY_REPAIR_BENCH label={label} old_median_ns={} old_samples_ns={old_samples:?} new_median_ns={} new_samples_ns={new_samples:?}",
            median(&old_samples),
            median(&new_samples),
        );
    }

    #[test]
    #[ignore = "temporary C2 quarantine-kernel benchmark"]
    fn quarantine_kernel_timing_probe() {
        for n in [128, 512, 2048, 8192] {
            let proportional_k = (n / 16).max(1);
            for k in [0, 1, 16, 64, proportional_k] {
                run_quarantine_kernel_probe(
                    &format!("quarantine_n{n}_k{k}"),
                    &malformed_bench_fixture(n, k),
                );
            }
        }
    }

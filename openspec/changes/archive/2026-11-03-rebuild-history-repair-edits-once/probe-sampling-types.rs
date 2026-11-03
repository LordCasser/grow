// Temporary ignored-test fixture and kernel probe source, copied from conversation.rs tests.
// It uses assistant_with_calls and the test-only old_* reference functions above it.

    fn dangling_bench_fixture(n: usize, k: usize) -> Vec<ConversationItem> {
        let mut items = Vec::with_capacity(n + k);
        for slot in 0..k {
            let target = slot * n / k;
            while items.len() < target {
                items.push(ConversationItem::user("filler"));
            }
            items.push(assistant_with_calls(&[(
                &format!("dangling-{slot}"),
                "read_file",
            )]));
        }
        while items.len() < n {
            items.push(ConversationItem::user("filler"));
        }
        items
    }

    fn duplicate_bench_fixture(n: usize, k: usize) -> Vec<ConversationItem> {
        let mut items = Vec::with_capacity(n + 3 * k);
        for slot in 0..k {
            let target = slot * n / k;
            while items.len() < target {
                items.push(ConversationItem::user("filler"));
            }
            let id = format!("duplicate-{slot}");
            items.push(assistant_with_calls(&[(&id, "read_file")]));
            items.push(ConversationItem::tool_result(&id, "stale result"));
            items.push(ConversationItem::tool_result(&id, "authoritative result"));
        }
        while items.len() < n {
            items.push(ConversationItem::user("filler"));
        }
        items
    }

    fn run_repair_kernel_probe(
        label: &str,
        original: &[ConversationItem],
        old: impl Fn(&mut Vec<ConversationItem>) -> usize,
        new: impl Fn(&mut Vec<ConversationItem>) -> usize,
    ) {
        let sample = |repair: &dyn Fn(&mut Vec<ConversationItem>) -> usize| {
            let mut items = original.to_vec();
            let started = std::time::Instant::now();
            let count = repair(&mut items);
            let elapsed = started.elapsed().as_nanos();
            let json = serde_json::to_vec(&items).unwrap();
            (elapsed, count, json)
        };
        for _ in 0..2 {
            let (_, old_count, old_json) = sample(&old);
            let (_, new_count, new_json) = sample(&new);
            assert_eq!(new_count, old_count);
            assert_eq!(new_json, old_json);
        }
        let mut old_samples = Vec::with_capacity(7);
        let mut new_samples = Vec::with_capacity(7);
        for _ in 0..7 {
            let (old_ns, old_count, old_json) = sample(&old);
            let (new_ns, new_count, new_json) = sample(&new);
            assert_eq!(new_count, old_count);
            assert_eq!(new_json, old_json);
            old_samples.push(old_ns);
            new_samples.push(new_ns);
        }
        let median = |samples: &[u128]| {
            let mut sorted = samples.to_vec();
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
    #[ignore = "temporary C2 repair-kernel benchmark"]
    fn history_repair_kernel_timing_probe() {
        for n in [128, 512, 2048, 8192] {
            let proportional_k = (n / 16).max(1);
            for k in [0, 1, 16, 64, proportional_k] {
                let dangling = dangling_bench_fixture(n, k);
                run_repair_kernel_probe(
                    &format!("dangling_n{n}_k{k}"),
                    &dangling,
                    |items| {
                        old_repair_dangling_tool_calls(items, DanglingToolCallReason::UserCancelled)
                    },
                    |items| {
                        repair_dangling_tool_calls(items, DanglingToolCallReason::UserCancelled)
                    },
                );
                let duplicates = duplicate_bench_fixture(n, k);
                run_repair_kernel_probe(
                    &format!("dedup_n{n}_k{k}"),
                    &duplicates,
                    old_dedup_duplicate_tool_results,
                    dedup_duplicate_tool_results,
                );
            }
        }
    }

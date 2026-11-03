# 纯函数测量方法与原始样本

本机debug test profile，隔离GROW_HOME。先在未改生产代码时运行，再在实现后运行；每个进程内同时保留旧reference和当前入口对照。2次warmup、7次样本，输入clone在计时外，输出drop在计时外；所有row预置worktree label，排除DB查询。每个规模先核对serialized完整输出。该fixture不是磁盘/DB/ACP/Pager端到端测量，不报告RSS、生产吞吐或SLA。

命令：`cargo test --locked -p shell --lib session::listing::tests -- --include-ignored --nocapture --test-threads=1`（RUST_MIN_STACK=16777216）。临时probe已从构建删除；下面代码可放回listing.rs的现有tests模块复跑，复用summary/old_listing_reference/serialized helper。

```rust
    fn benchmark_summaries(count: usize) -> Vec<Summary> {
        let base = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        (0..count)
            .map(|index| {
                let mut row = summary(
                    &format!("bench-{index:05}"),
                    &format!("/repo/worktree-{}", index % 16),
                    &format!("benchmark session {index}"),
                    index % 11,
                );
                row.updated_at = base + Duration::seconds(index as i64);
                if index % 3 != 0 {
                    row.last_active_at = Some(row.updated_at + Duration::nanoseconds(17));
                }
                // Avoid worktree DB lookups: the pure probe measures listing
                // materialization and ordering only.
                row.worktree_label = Some(format!("worktree-{}", index % 16));
                row
            })
            .collect()
    }

    fn timing_samples(
        input: &[Summary],
        run: impl Fn(Vec<Summary>) -> Vec<SessionListing>,
    ) -> Vec<u128> {
        for _ in 0..2 {
            std::hint::black_box(run(input.to_vec()));
        }
        let mut samples = Vec::with_capacity(7);
        for _ in 0..7 {
            let cloned_input = input.to_vec();
            let start = std::time::Instant::now();
            let output = run(std::hint::black_box(cloned_input));
            samples.push(start.elapsed().as_nanos());
            std::hint::black_box(output);
        }
        samples
    }

    #[test]
    #[ignore = "temporary pure-function timing probe; run explicitly for before/after comparison"]
    fn listing_materialization_timing_probe() {
        for count in [128, 512, 2048, 9864] {
            let input = benchmark_summaries(count);
            let reference = old_listing_reference(input.clone(), None, 30);
            let current = list_from_summaries(input.clone(), None, 30);
            assert_eq!(serialized(&current), serialized(&reference));

            let old_samples = timing_samples(&input, |rows| old_listing_reference(rows, None, 30));
            let current_samples =
                timing_samples(&input, |rows| list_from_summaries(rows, None, 30));
            let mut old_sorted = old_samples.clone();
            let mut current_sorted = current_samples.clone();
            old_sorted.sort_unstable();
            current_sorted.sort_unstable();
            eprintln!(
                "listing pure probe: summaries={count}, limit=30, warmups=2, repetitions=7, old_median_ns={}, old_samples_ns={old_samples:?}, current_median_ns={}, current_samples_ns={current_samples:?}",
                old_sorted[old_sorted.len() / 2],
                current_sorted[current_sorted.len() / 2],
            );
        }
    }


```

## 实施前

```text
test session::listing::tests::listing_materialization_timing_probe ... listing pure probe: summaries=128, limit=30, warmups=2, repetitions=7, old_median_ns=345541, old_samples_ns=[201750, 333583, 346875, 349833, 345541, 344625, 345791], current_median_ns=346750, current_samples_ns=[204416, 205459, 306875, 346750, 347250, 348583, 348333]
listing pure probe: summaries=512, limit=30, warmups=2, repetitions=7, old_median_ns=1141833, old_samples_ns=[1142833, 1139458, 1143834, 1141833, 1141833, 1138333, 1120916], current_median_ns=1013375, current_samples_ns=[1061959, 1062541, 1057583, 1013375, 989916, 998458, 995583]
listing pure probe: summaries=2048, limit=30, warmups=2, repetitions=7, old_median_ns=3287875, old_samples_ns=[3472000, 3390666, 3334417, 3287875, 3227250, 3206334, 3163584], current_median_ns=3109084, current_samples_ns=[3013708, 3060166, 3095917, 3112625, 3125042, 3138250, 3109084]
listing pure probe: summaries=9864, limit=30, warmups=2, repetitions=7, old_median_ns=15144625, old_samples_ns=[15166209, 15144167, 15144625, 15222333, 15154583, 14270417, 14970875], current_median_ns=15015084, current_samples_ns=[15124959, 15059083, 15228916, 14665542, 14786584, 14976583, 15015084]
```

## 实施后

```text
test session::listing::tests::listing_materialization_timing_probe ... listing pure probe: summaries=128, limit=30, warmups=2, repetitions=7, old_median_ns=307541, old_samples_ns=[342167, 309042, 305208, 307791, 307541, 307042, 306291], current_median_ns=93291, current_samples_ns=[93417, 93291, 93375, 93167, 93250, 92917, 94291]
listing pure probe: summaries=512, limit=30, warmups=2, repetitions=7, old_median_ns=1103500, old_samples_ns=[1132875, 1130625, 1131667, 1103500, 1051250, 1051792, 1046292], current_median_ns=223584, current_samples_ns=[224750, 223667, 222917, 223458, 223500, 223584, 223875]
listing pure probe: summaries=2048, limit=30, warmups=2, repetitions=7, old_median_ns=3345042, old_samples_ns=[3553375, 3531292, 3413458, 3345042, 3279000, 3237750, 3231833], current_median_ns=585584, current_samples_ns=[613250, 585167, 599500, 585125, 583875, 585584, 587041]
listing pure probe: summaries=9864, limit=30, warmups=2, repetitions=7, old_median_ns=14937208, old_samples_ns=[14937208, 15020875, 14974750, 14565000, 14460167, 14679292, 14997042], current_median_ns=2782458, current_samples_ns=[2821959, 2785541, 2749333, 2798625, 2740000, 2782458, 2774417]
```

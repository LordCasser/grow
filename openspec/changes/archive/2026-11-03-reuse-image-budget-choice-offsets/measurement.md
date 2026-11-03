# 图片预算定位内核测量

本机debug test profile，隔离GROW_HOME、RUST_MIN_STACK=16777216。before使用原SID-key生产函数，after使用offset-key生产函数；每个进程都保留独立legacy Vec oracle作控制（Vec迭代不等于旧BTreeMap迭代，不能把两者常数成本等同）。同样fixture、2 warmups/7 samples，items clone在计时外，纯测validation+apply；不测prefix比较、request clone、预算bytecount、effective-parts reporting或编码/provider。

N=128/2048/8192，K/D=0/1/2/8/64，以及D1/K16。未选slot是普通text；front图片前置、tail图片位于长text前缀后，每组先做全serialized items差分。只有front的早期探针不足以观察N维度，正式before已补front/tail再采集。初始fixture把默认User的Text part误当image0，修正为显式image-only后再执行；未修改生产/预期掩盖失败。

命令：`cargo test --locked -p chat-state --lib image_budget_choice -- --include-ignored --nocapture --test-threads=1`。临时probe已从构建删除，以下片段可放回request_builder.rs::tests复跑，复用最终保留的fixture与legacy helper。

```rust
    fn legacy_request_kernel(
        items: &mut [ConversationItem],
        ids: &[crate::SurfaceId],
        choices: &[LegacyChoice],
    ) {
        if legacy_choices_are_valid(items, ids, choices) {
            legacy_apply_choices(items, ids, choices);
        }
    }

    fn actual_request_kernel(
        items: &mut [ConversationItem],
        ids: &[crate::SurfaceId],
        selection: &ImageBudgetSelection,
    ) {
        let valid = image_budget_choices_are_valid(items, ids, selection);
        if valid {
            apply_image_budget_choices(items, ids, selection);
        }
    }

    fn timing_samples(
        items: &[ConversationItem],
        run: impl Fn(&mut [ConversationItem]),
    ) -> Vec<u128> {
        for _ in 0..2 {
            let mut working = items.to_vec();
            run(&mut working);
            std::hint::black_box(working);
        }
        let mut samples = Vec::with_capacity(7);
        for _ in 0..7 {
            let mut working = items.to_vec();
            let start = std::time::Instant::now();
            run(&mut working);
            samples.push(start.elapsed().as_nanos());
            std::hint::black_box(working);
        }
        samples
    }

    fn benchmark_selection(
        count: usize,
        first_source: usize,
        distinct_sources: usize,
        parts_on_one_source: usize,
    ) -> (
        Vec<ConversationItem>,
        Vec<crate::SurfaceId>,
        ImageBudgetSelection,
        Vec<LegacyChoice>,
    ) {
        let mut part_counts = vec![0; count];
        if distinct_sources == 1 && parts_on_one_source > 1 {
            part_counts[first_source] = parts_on_one_source;
        } else {
            for source in first_source..first_source + distinct_sources {
                part_counts[source] = 1;
            }
        }
        let items = image_items(&part_counts);
        let ids = surface_ids(count);
        let mut specs = Vec::new();
        if distinct_sources == 1 && parts_on_one_source > 1 {
            for part in 0..parts_on_one_source {
                specs.push((first_source, part, first_source, part));
            }
        } else {
            for source in first_source..first_source + distinct_sources {
                specs.push((source, 0, source, 0));
            }
        }
        let (selection, legacy) = selection_and_legacy_choices(&items, &ids, &specs);
        (items, ids, selection, legacy)
    }

    fn median_and_samples(
        count: usize,
        layout: &str,
        first_source: usize,
        items: &[ConversationItem],
        ids: &[crate::SurfaceId],
        selection: &ImageBudgetSelection,
        legacy: &[LegacyChoice],
    ) {
        let actual_samples = timing_samples(items, |working| {
            actual_request_kernel(working, ids, selection)
        });
        let legacy_samples =
            timing_samples(items, |working| legacy_request_kernel(working, ids, legacy));
        let mut actual_sorted = actual_samples.clone();
        let mut legacy_sorted = legacy_samples.clone();
        actual_sorted.sort_unstable();
        legacy_sorted.sort_unstable();
        eprintln!(
            "image-choice kernel: layout={layout}, N={count}, first_source={first_source}, K={}, D={}, actual_median_ns={}, actual_samples_ns={actual_samples:?}, legacy_median_ns={}, legacy_samples_ns={legacy_samples:?}",
            selection.replaced.len(),
            selection
                .replaced
                .keys()
                .map(|(source, _)| *source)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            actual_sorted[actual_sorted.len() / 2],
            legacy_sorted[legacy_sorted.len() / 2],
        );
    }

    #[test]
    #[ignore = "temporary image-choice kernel timing probe; run explicitly for before/after comparison"]
    fn image_budget_choice_timing_probe() {
        for count in [128, 2048, 8192] {
            for distinct_sources in [0, 1, 2, 8, 64] {
                let first_sources = if distinct_sources == 0 {
                    vec![0]
                } else {
                    vec![0, count - distinct_sources]
                };
                for (layout, first_source) in
                    first_sources
                        .iter()
                        .enumerate()
                        .map(|(index, first_source)| {
                            (if index == 0 { "front" } else { "tail" }, *first_source)
                        })
                {
                    let (items, ids, selection, legacy) =
                        benchmark_selection(count, first_source, distinct_sources, 1);
                    assert!(image_budget_choices_are_valid(&items, &ids, &selection));
                    let mut actual = items.clone();
                    let mut expected = items.clone();
                    actual_request_kernel(&mut actual, &ids, &selection);
                    legacy_request_kernel(&mut expected, &ids, &legacy);
                    assert_eq!(serialized_items(&actual), serialized_items(&expected));
                    median_and_samples(
                        count,
                        layout,
                        first_source,
                        &items,
                        &ids,
                        &selection,
                        &legacy,
                    );
                }
            }

            for (layout, first_source) in [("front-multipart", 0), ("tail-multipart", count - 1)] {
                let (items, ids, selection, legacy) =
                    benchmark_selection(count, first_source, 1, 16);
                assert!(image_budget_choices_are_valid(&items, &ids, &selection));
                let mut actual = items.clone();
                let mut expected = items.clone();
                actual_request_kernel(&mut actual, &ids, &selection);
                legacy_request_kernel(&mut expected, &ids, &legacy);
                assert_eq!(serialized_items(&actual), serialized_items(&expected));
                median_and_samples(
                    count,
                    layout,
                    first_source,
                    &items,
                    &ids,
                    &selection,
                    &legacy,
                );
            }
        }
    }
```

## Before原始样本

```text
test actor::request_builder::tests::image_budget_choice_timing_probe ... image-choice kernel: layout=front, N=128, first_source=0, K=0, D=0, actual_median_ns=83, actual_samples_ns=[83, 83, 83, 41, 42, 42, 84], legacy_median_ns=42, legacy_samples_ns=[83, 84, 42, 83, 42, 42, 42]
image-choice kernel: layout=front, N=128, first_source=0, K=1, D=1, actual_median_ns=625, actual_samples_ns=[583, 2959, 750, 625, 625, 625, 625], legacy_median_ns=416, legacy_samples_ns=[375, 375, 416, 417, 375, 416, 417]
image-choice kernel: layout=tail, N=128, first_source=127, K=1, D=1, actual_median_ns=2250, actual_samples_ns=[2250, 2292, 2250, 2291, 2209, 2291, 2167], legacy_median_ns=2041, legacy_samples_ns=[6417, 2041, 2000, 2166, 2041, 2000, 2000]
image-choice kernel: layout=front, N=128, first_source=0, K=2, D=2, actual_median_ns=1042, actual_samples_ns=[1167, 1083, 1042, 1042, 1000, 1084, 1042], legacy_median_ns=667, legacy_samples_ns=[709, 667, 667, 667, 625, 625, 666]
image-choice kernel: layout=tail, N=128, first_source=126, K=2, D=2, actual_median_ns=3833, actual_samples_ns=[3792, 3833, 3833, 3791, 3917, 3834, 3791], legacy_median_ns=3458, legacy_samples_ns=[3500, 3416, 3458, 3458, 3500, 3417, 3500]
image-choice kernel: layout=front, N=128, first_source=0, K=8, D=8, actual_median_ns=3625, actual_samples_ns=[3750, 3708, 3459, 3625, 3625, 3625, 3708], legacy_median_ns=2583, legacy_samples_ns=[2500, 2667, 2584, 2583, 2625, 2542, 2583]
image-choice kernel: layout=tail, N=128, first_source=120, K=8, D=8, actual_median_ns=13917, actual_samples_ns=[13792, 14000, 13917, 13875, 14000, 13958, 13708], legacy_median_ns=12875, legacy_samples_ns=[12916, 12875, 12875, 12917, 12834, 12917, 12792]
image-choice kernel: layout=front, N=128, first_source=0, K=64, D=64, actual_median_ns=43208, actual_samples_ns=[43250, 43208, 43291, 43042, 43166, 42792, 43250], legacy_median_ns=35625, legacy_samples_ns=[35750, 35125, 38125, 35542, 35625, 35708, 35500]
image-choice kernel: layout=tail, N=128, first_source=64, K=64, D=64, actual_median_ns=81166, actual_samples_ns=[81292, 81917, 81166, 80791, 80833, 81500, 80750], legacy_median_ns=73416, legacy_samples_ns=[73667, 73416, 73333, 73208, 73417, 73875, 73125]
image-choice kernel: layout=front-multipart, N=128, first_source=0, K=16, D=1, actual_median_ns=4916, actual_samples_ns=[4958, 5000, 4875, 4875, 4916, 4958, 4792], legacy_median_ns=3208, legacy_samples_ns=[3125, 3250, 3208, 3209, 3167, 3292, 3167]
image-choice kernel: layout=tail-multipart, N=128, first_source=127, K=16, D=1, actual_median_ns=23208, actual_samples_ns=[23000, 23333, 23250, 23167, 23250, 23208, 23125], legacy_median_ns=21583, legacy_samples_ns=[21542, 21667, 21583, 21667, 21542, 21708, 21333]
image-choice kernel: layout=front, N=2048, first_source=0, K=0, D=0, actual_median_ns=42, actual_samples_ns=[83, 42, 41, 42, 42, 41, 41], legacy_median_ns=42, legacy_samples_ns=[0, 42, 41, 42, 0, 42, 42]
image-choice kernel: layout=front, N=2048, first_source=0, K=1, D=1, actual_median_ns=334, actual_samples_ns=[375, 334, 334, 334, 375, 333, 375], legacy_median_ns=250, legacy_samples_ns=[250, 250, 250, 250, 250, 250, 250]
image-choice kernel: layout=tail, N=2048, first_source=2047, K=1, D=1, actual_median_ns=12667, actual_samples_ns=[12417, 12500, 13000, 13250, 13125, 12333, 12667], legacy_median_ns=13959, legacy_samples_ns=[13959, 13917, 14167, 14000, 14458, 13917, 13750]
image-choice kernel: layout=front, N=2048, first_source=0, K=2, D=2, actual_median_ns=500, actual_samples_ns=[583, 542, 500, 500, 500, 500, 541], legacy_median_ns=334, legacy_samples_ns=[333, 334, 334, 333, 375, 375, 375]
image-choice kernel: layout=tail, N=2048, first_source=2046, K=2, D=2, actual_median_ns=22917, actual_samples_ns=[23291, 23041, 22792, 22917, 23416, 22875, 22916], legacy_median_ns=25084, legacy_samples_ns=[25250, 25000, 25084, 25375, 25041, 25208, 25000]
image-choice kernel: layout=front, N=2048, first_source=0, K=8, D=8, actual_median_ns=2041, actual_samples_ns=[2000, 1917, 2042, 2000, 2042, 2042, 2041], legacy_median_ns=1417, legacy_samples_ns=[1417, 1500, 1458, 1459, 1333, 1416, 1333]
image-choice kernel: layout=tail, N=2048, first_source=2040, K=8, D=8, actual_median_ns=104000, actual_samples_ns=[103250, 106250, 104000, 104125, 112750, 89709, 89541], legacy_median_ns=92417, legacy_samples_ns=[92459, 92500, 92583, 92417, 92209, 92250, 92208]
image-choice kernel: layout=front, N=2048, first_source=0, K=64, D=64, actual_median_ns=25875, actual_samples_ns=[26250, 26042, 25792, 26083, 25833, 25792, 25875], legacy_median_ns=20875, legacy_samples_ns=[20917, 21000, 20833, 20875, 20833, 20875, 23416]
image-choice kernel: layout=tail, N=2048, first_source=1984, K=64, D=64, actual_median_ns=744292, actual_samples_ns=[839292, 798834, 736708, 744834, 743083, 743666, 744292], legacy_median_ns=764125, legacy_samples_ns=[761417, 763542, 767458, 770250, 767083, 764125, 761083]
image-choice kernel: layout=front-multipart, N=2048, first_source=0, K=16, D=1, actual_median_ns=3125, actual_samples_ns=[3167, 3125, 2958, 3084, 3167, 3125, 2958], legacy_median_ns=2083, legacy_samples_ns=[2083, 2084, 2125, 2042, 2291, 2000, 2042]
image-choice kernel: layout=tail-multipart, N=2048, first_source=2047, K=16, D=1, actual_median_ns=182542, actual_samples_ns=[187208, 182542, 182250, 180667, 202834, 181167, 191250], legacy_median_ns=189834, legacy_samples_ns=[206000, 194708, 189834, 185958, 188667, 201292, 185375]
image-choice kernel: layout=front, N=8192, first_source=0, K=0, D=0, actual_median_ns=42, actual_samples_ns=[41, 42, 42, 0, 42, 41, 42], legacy_median_ns=0, legacy_samples_ns=[0, 42, 41, 0, 42, 0, 0]
image-choice kernel: layout=front, N=8192, first_source=0, K=1, D=1, actual_median_ns=375, actual_samples_ns=[459, 375, 3583, 375, 375, 375, 333], legacy_median_ns=291, legacy_samples_ns=[333, 291, 209, 208, 250, 292, 459]
image-choice kernel: layout=tail, N=8192, first_source=8191, K=1, D=1, actual_median_ns=46500, actual_samples_ns=[46834, 51709, 46208, 46500, 46708, 46292, 46125], legacy_median_ns=46250, legacy_samples_ns=[51292, 45875, 46416, 45834, 45834, 51708, 46250]
image-choice kernel: layout=front, N=8192, first_source=0, K=2, D=2, actual_median_ns=541, actual_samples_ns=[542, 541, 500, 584, 500, 542, 500], legacy_median_ns=375, legacy_samples_ns=[333, 375, 375, 334, 375, 333, 375]
image-choice kernel: layout=tail, N=8192, first_source=8190, K=2, D=2, actual_median_ns=91375, actual_samples_ns=[95459, 91625, 91125, 91042, 91041, 91375, 91959], legacy_median_ns=98625, legacy_samples_ns=[98541, 98500, 98625, 98542, 98666, 98625, 98667]
image-choice kernel: layout=front, N=8192, first_source=0, K=8, D=8, actual_median_ns=2042, actual_samples_ns=[2000, 2250, 2042, 2083, 2000, 1917, 2042], legacy_median_ns=1458, legacy_samples_ns=[1458, 1458, 1417, 1417, 1625, 1583, 1459]
image-choice kernel: layout=tail, N=8192, first_source=8184, K=8, D=8, actual_median_ns=396958, actual_samples_ns=[406875, 396958, 373542, 377917, 399542, 398125, 372042], legacy_median_ns=379958, legacy_samples_ns=[379958, 404709, 389125, 393041, 373458, 377375, 372958]
image-choice kernel: layout=front, N=8192, first_source=0, K=64, D=64, actual_median_ns=26667, actual_samples_ns=[26791, 26667, 26792, 26834, 26584, 26625, 26500], legacy_median_ns=22667, legacy_samples_ns=[25167, 22500, 22750, 22667, 25291, 22583, 22541]
image-choice kernel: layout=tail, N=8192, first_source=8128, K=64, D=64, actual_median_ns=3133042, actual_samples_ns=[3087000, 3123875, 3137125, 3129792, 3133250, 3133042, 3142750], legacy_median_ns=3142625, legacy_samples_ns=[3143833, 3124000, 3136541, 3132167, 3159250, 3142625, 3161875]
image-choice kernel: layout=front-multipart, N=8192, first_source=0, K=16, D=1, actual_median_ns=3208, actual_samples_ns=[3125, 3250, 3167, 3334, 3208, 3209, 3208], legacy_median_ns=2167, legacy_samples_ns=[2166, 2208, 2125, 2375, 2375, 2125, 2167]
image-choice kernel: layout=tail-multipart, N=8192, first_source=8191, K=16, D=1, actual_median_ns=792333, actual_samples_ns=[797166, 769083, 786917, 797000, 771625, 792333, 800375], legacy_median_ns=797292, legacy_samples_ns=[796584, 781000, 804167, 805834, 781417, 805584, 797292]
```

## After原始样本

```text
test actor::request_builder::tests::image_budget_choice_timing_probe ... image-choice kernel: layout=front, N=128, first_source=0, K=0, D=0, actual_median_ns=83, actual_samples_ns=[83, 42, 42, 84, 83, 84, 42], legacy_median_ns=42, legacy_samples_ns=[41, 0, 42, 0, 42, 42, 42]
image-choice kernel: layout=front, N=128, first_source=0, K=1, D=1, actual_median_ns=458, actual_samples_ns=[417, 458, 458, 458, 2167, 500, 500], legacy_median_ns=292, legacy_samples_ns=[208, 292, 292, 291, 292, 292, 250]
image-choice kernel: layout=tail, N=128, first_source=127, K=1, D=1, actual_median_ns=458, actual_samples_ns=[458, 500, 459, 458, 458, 458, 417], legacy_median_ns=1500, legacy_samples_ns=[1542, 1500, 1500, 1500, 1500, 1459, 1500]
image-choice kernel: layout=front, N=128, first_source=0, K=2, D=2, actual_median_ns=750, actual_samples_ns=[791, 709, 750, 667, 709, 750, 791], legacy_median_ns=459, legacy_samples_ns=[500, 542, 500, 458, 458, 459, 458]
image-choice kernel: layout=tail, N=128, first_source=126, K=2, D=2, actual_median_ns=709, actual_samples_ns=[750, 709, 709, 709, 708, 667, 708], legacy_median_ns=2666, legacy_samples_ns=[2750, 2709, 2666, 2709, 2625, 2666, 2666]
image-choice kernel: layout=front, N=128, first_source=0, K=8, D=8, actual_median_ns=2459, actual_samples_ns=[2500, 2500, 2500, 2417, 2458, 2333, 2459], legacy_median_ns=2042, legacy_samples_ns=[2250, 2042, 2042, 1958, 2083, 2042, 2042]
image-choice kernel: layout=tail, N=128, first_source=120, K=8, D=8, actual_median_ns=2375, actual_samples_ns=[2375, 2500, 2334, 2375, 2250, 2459, 2417], legacy_median_ns=9916, legacy_samples_ns=[9750, 9959, 10125, 9833, 9959, 9667, 9916]
image-choice kernel: layout=front, N=128, first_source=0, K=64, D=64, actual_median_ns=18958, actual_samples_ns=[18958, 19208, 18916, 19292, 18875, 18917, 19083], legacy_median_ns=30166, legacy_samples_ns=[30125, 30500, 30292, 30000, 30166, 30000, 30375]
image-choice kernel: layout=tail, N=128, first_source=64, K=64, D=64, actual_median_ns=17708, actual_samples_ns=[17042, 17958, 17750, 18083, 16916, 17708, 16917], legacy_median_ns=58958, legacy_samples_ns=[58666, 59250, 58417, 58750, 59250, 58958, 59083]
image-choice kernel: layout=front-multipart, N=128, first_source=0, K=16, D=1, actual_median_ns=4583, actual_samples_ns=[4667, 4542, 4583, 4375, 4584, 4416, 4625], legacy_median_ns=2834, legacy_samples_ns=[2875, 2834, 2791, 2833, 2916, 2916, 2833]
image-choice kernel: layout=tail-multipart, N=128, first_source=127, K=16, D=1, actual_median_ns=4541, actual_samples_ns=[4542, 4542, 4500, 4458, 4541, 4542, 4500], legacy_median_ns=18833, legacy_samples_ns=[18750, 18834, 18833, 18833, 18875, 18875, 18708]
image-choice kernel: layout=front, N=2048, first_source=0, K=0, D=0, actual_median_ns=41, actual_samples_ns=[42, 41, 0, 42, 41, 41, 0], legacy_median_ns=41, legacy_samples_ns=[41, 41, 0, 0, 42, 0, 41]
image-choice kernel: layout=front, N=2048, first_source=0, K=1, D=1, actual_median_ns=375, actual_samples_ns=[333, 375, 375, 375, 375, 375, 375], legacy_median_ns=250, legacy_samples_ns=[250, 250, 250, 208, 250, 250, 250]
image-choice kernel: layout=tail, N=2048, first_source=2047, K=1, D=1, actual_median_ns=333, actual_samples_ns=[333, 333, 333, 333, 333, 334, 334], legacy_median_ns=11750, legacy_samples_ns=[11833, 11625, 11709, 11750, 11792, 11666, 11875]
image-choice kernel: layout=front, N=2048, first_source=0, K=2, D=2, actual_median_ns=542, actual_samples_ns=[542, 583, 542, 500, 542, 542, 541], legacy_median_ns=333, legacy_samples_ns=[375, 375, 292, 333, 333, 333, 375]
image-choice kernel: layout=tail, N=2048, first_source=2046, K=2, D=2, actual_median_ns=500, actual_samples_ns=[500, 500, 500, 541, 542, 500, 500], legacy_median_ns=22500, legacy_samples_ns=[22250, 22625, 22375, 22875, 22334, 22500, 22500]
image-choice kernel: layout=front, N=2048, first_source=0, K=8, D=8, actual_median_ns=1750, actual_samples_ns=[1709, 1834, 1792, 1792, 1750, 1750, 1750], legacy_median_ns=1458, legacy_samples_ns=[1500, 1416, 1458, 1417, 1500, 1458, 1375]
image-choice kernel: layout=tail, N=2048, first_source=2040, K=8, D=8, actual_median_ns=1708, actual_samples_ns=[1750, 1708, 1708, 1708, 1625, 1667, 1750], legacy_median_ns=89041, legacy_samples_ns=[91250, 89041, 93458, 91167, 87041, 87833, 88750]
image-choice kernel: layout=front, N=2048, first_source=0, K=64, D=64, actual_median_ns=13250, actual_samples_ns=[13250, 13250, 13333, 13166, 13167, 13083, 13292], legacy_median_ns=20958, legacy_samples_ns=[20958, 20834, 21000, 20959, 20875, 21167, 20750]
image-choice kernel: layout=tail, N=2048, first_source=1984, K=64, D=64, actual_median_ns=13250, actual_samples_ns=[12625, 13083, 13334, 12958, 21291, 13292, 13250], legacy_median_ns=758792, legacy_samples_ns=[722958, 759167, 758792, 759125, 749916, 759666, 756292]
image-choice kernel: layout=front-multipart, N=2048, first_source=0, K=16, D=1, actual_median_ns=3417, actual_samples_ns=[3250, 3459, 3250, 3250, 3417, 3417, 3583], legacy_median_ns=2083, legacy_samples_ns=[2167, 2375, 2083, 2000, 2042, 2000, 2333]
image-choice kernel: layout=tail-multipart, N=2048, first_source=2047, K=16, D=1, actual_median_ns=3375, actual_samples_ns=[3459, 3375, 3375, 3583, 3292, 3250, 3334], legacy_median_ns=181334, legacy_samples_ns=[176250, 178375, 186709, 194042, 188333, 181334, 179875]
image-choice kernel: layout=front, N=8192, first_source=0, K=0, D=0, actual_median_ns=42, actual_samples_ns=[83, 83, 41, 41, 41, 42, 83], legacy_median_ns=41, legacy_samples_ns=[0, 41, 0, 0, 41, 42, 42]
image-choice kernel: layout=front, N=8192, first_source=0, K=1, D=1, actual_median_ns=375, actual_samples_ns=[417, 375, 416, 375, 416, 334, 334], legacy_median_ns=250, legacy_samples_ns=[250, 250, 250, 209, 209, 292, 250]
image-choice kernel: layout=tail, N=8192, first_source=8191, K=1, D=1, actual_median_ns=375, actual_samples_ns=[416, 333, 375, 375, 375, 375, 417], legacy_median_ns=46625, legacy_samples_ns=[46625, 46292, 52000, 46334, 46000, 46750, 46916]
image-choice kernel: layout=front, N=8192, first_source=0, K=2, D=2, actual_median_ns=584, actual_samples_ns=[708, 625, 583, 625, 584, 542, 542], legacy_median_ns=375, legacy_samples_ns=[375, 375, 375, 375, 416, 417, 417]
image-choice kernel: layout=tail, N=8192, first_source=8190, K=2, D=2, actual_median_ns=541, actual_samples_ns=[625, 541, 541, 541, 541, 500, 583], legacy_median_ns=91417, legacy_samples_ns=[108834, 91417, 91416, 91792, 91375, 91250, 91417]
image-choice kernel: layout=front, N=8192, first_source=0, K=8, D=8, actual_median_ns=1791, actual_samples_ns=[2042, 1708, 1792, 1791, 1750, 1792, 1750], legacy_median_ns=1500, legacy_samples_ns=[1542, 1500, 1500, 1542, 1500, 1500, 1708]
image-choice kernel: layout=tail, N=8192, first_source=8184, K=8, D=8, actual_median_ns=1708, actual_samples_ns=[1750, 1791, 1667, 1750, 1667, 1708, 1708], legacy_median_ns=387458, legacy_samples_ns=[400542, 387458, 368625, 387416, 386000, 388250, 389542]
image-choice kernel: layout=front, N=8192, first_source=0, K=64, D=64, actual_median_ns=13375, actual_samples_ns=[12541, 13375, 13250, 14750, 14792, 12625, 13417], legacy_median_ns=20791, legacy_samples_ns=[20542, 20792, 20584, 20917, 20750, 20875, 20791]
image-choice kernel: layout=tail, N=8192, first_source=8128, K=64, D=64, actual_median_ns=13000, actual_samples_ns=[14542, 13000, 12333, 13084, 12917, 12834, 14417], legacy_median_ns=3098583, legacy_samples_ns=[3080333, 3094000, 3112333, 3099916, 3113500, 3097292, 3098583]
image-choice kernel: layout=front-multipart, N=8192, first_source=0, K=16, D=1, actual_median_ns=3458, actual_samples_ns=[3583, 3875, 3750, 3417, 3458, 3458, 3458], legacy_median_ns=2042, legacy_samples_ns=[2291, 2000, 2042, 2042, 2041, 2083, 2083]
image-choice kernel: layout=tail-multipart, N=8192, first_source=8191, K=16, D=1, actual_median_ns=3417, actual_samples_ns=[6167, 3416, 3417, 3333, 3417, 3375, 3750], legacy_median_ns=785000, legacy_samples_ns=[782250, 785833, 785000, 794084, 795375, 779292, 781958]
```

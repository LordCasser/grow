
#[test]
#[ignore = "manual timing probe"]
fn measure_managed_plan_batches() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("config.rc");
    fs::write(&path, "x".repeat(1024 * 1024)).unwrap();
    for count in [1, 16, 64] {
        let mut elapsed = Vec::new();
        for _ in 0..3 {
            let mut input = request(&path, &[("seed", "body")]);
            input.items = (0..count).map(|index| ManagedItem::new(
                format!("terminal.item{index}"), "body"
            )).collect();
            let started = Instant::now();
            let plan = ManagedConfig::plan(input).unwrap();
            elapsed.push(started.elapsed().as_micros());
            assert!(plan.updated_bytes().len() > 1024 * 1024);
        }
        elapsed.sort();
        eprintln!("MANAGED_PLAN count={count} median_us={} samples_us={elapsed:?}", elapsed[1]);
    }
}

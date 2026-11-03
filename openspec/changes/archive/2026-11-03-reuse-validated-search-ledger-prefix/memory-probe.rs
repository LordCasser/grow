// Temporary ignored test body run with the cfg(test) CountingAlloc in
// crates/codegen/shell/src/lib.rs. Run only this exact test in one process.
// First restore only search_benchmark_timeline from probe.rs into search.rs::tests;
// that temporary helper is no longer in the build. Then paste this test there.
// encode_timeline and cached_projection remain in search.rs::tests. Install
// memory-allocator-probe.rs in shell/src/lib.rs for this diagnostic only, and
// remove both additions after the one exact memory test (not the timing probe).

#[test]
#[ignore = "temporary S2 retained-allocation probe"]
fn search_projection_retained_allocation_probe() {
    for turns in [128, 512, 2048] {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("timeline.jsonl");
        let timeline = search_benchmark_timeline(turns, "aaaa");
        let bytes = encode_timeline(&timeline);
        let timeline_events = timeline.events().len();
        let physical_records = bytes.iter().filter(|byte| **byte == b'\n').count();
        std::fs::write(&path, &bytes).unwrap();

        // Warm file handling and one projection lifecycle. Fixture allocations
        // remain live and cancel from each before/after delta.
        drop(cached_projection(&path, None).unwrap());
        let before = crate::backlog_search_alloc::live();
        let projection = cached_projection(&path, None).unwrap();
        let after_projection = crate::backlog_search_alloc::live();
        let retained = after_projection.saturating_sub(before);
        drop(projection);
        let after_drop = crate::backlog_search_alloc::live();
        eprintln!(
            "SEARCH_PROJECTION_MEMORY turns={turns} serialized_payload_bytes={} timeline_events={timeline_events} physical_records={physical_records} live_before={before} live_after_projection={after_projection} retained_allocation_bytes={retained} live_after_drop={after_drop} drop_released_bytes={}",
            bytes.len(),
            after_projection.saturating_sub(after_drop),
        );
    }
}

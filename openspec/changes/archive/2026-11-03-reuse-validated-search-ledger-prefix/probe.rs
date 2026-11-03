// Exact temporary ignored-test body used for the timing run.
// Paste inside `session::storage::search::tests`; it uses that module's
// `record_search_turn`, `encode_timeline`, `full_projection`,
// `cached_projection`, and `timeline_indexable_content` helpers.

fn search_benchmark_timeline(turns: usize, marker: &str) -> Timeline {
    let mut timeline = Timeline::default();
    for index in 0..turns {
        let prompt = format!("prompt-{index:04}-{marker} payload");
        let answer = format!("answer-{index:04}-{marker} payload");
        record_search_turn(
            &mut timeline,
            index as u64 + 1,
            index,
            &prompt,
            &answer,
            Vec::new(),
        );
    }
    timeline
}

fn report_search_samples(label: &str, expected: &str, mut run: impl FnMut() -> (u128, String)) {
    for _ in 0..2 {
        let (_, content) = run();
        assert_eq!(content, expected, "warmup output mismatch: {label}");
    }
    let mut samples = Vec::with_capacity(7);
    for _ in 0..7 {
        let (elapsed, content) = run();
        assert_eq!(content, expected, "measured output mismatch: {label}");
        samples.push(elapsed);
    }
    let mut sorted = samples.clone();
    sorted.sort_unstable();
    eprintln!(
        "SEARCH_PROJECTION_BENCH label={label} median_ns={} samples_ns={samples:?}",
        sorted[sorted.len() / 2]
    );
}

#[test]
#[ignore = "temporary S2 benchmark; run explicitly for the change measurement"]
fn search_projection_timing_probe() {
    for turns in [128, 512, 2048] {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("timeline.jsonl");
        let base = search_benchmark_timeline(turns, "aaaa");
        let base_bytes = encode_timeline(&base);
        let base_expected = timeline_indexable_content(&base);
        std::fs::write(&path, &base_bytes).unwrap();
        eprintln!(
            "SEARCH_PROJECTION_FIXTURE turns={turns} serialized_payload_bytes={}",
            base_bytes.len()
        );

        report_search_samples(&format!("old_full_{turns}"), &base_expected, || {
            let started = std::time::Instant::now();
            let output = full_projection(&path).unwrap().0;
            (started.elapsed().as_nanos(), output)
        });
        report_search_samples(&format!("cold_projection_{turns}"), &base_expected, || {
            let started = std::time::Instant::now();
            let projection = cached_projection(&path, None).unwrap();
            let output = timeline_indexable_content(&projection.timeline);
            (started.elapsed().as_nanos(), output)
        });

        let mut retained = Some(cached_projection(&path, None).unwrap());
        report_search_samples(&format!("unchanged_{turns}"), &base_expected, || {
            let started = std::time::Instant::now();
            let previous = retained.take();
            let projection = cached_projection(&path, previous).unwrap();
            let output = timeline_indexable_content(&projection.timeline);
            let elapsed = started.elapsed().as_nanos();
            retained = Some(projection);
            (elapsed, output)
        });

        let mut appended = base.clone();
        record_search_turn(
            &mut appended,
            turns as u64 + 1,
            turns,
            "one appended tail",
            "one appended answer",
            Vec::new(),
        );
        let appended_bytes = encode_timeline(&appended);
        assert!(appended_bytes.starts_with(&base_bytes));
        let appended_expected = timeline_indexable_content(&appended);
        report_search_samples(&format!("append_one_turn_{turns}"), &appended_expected, || {
            // Checkpoint construction and fixture writes are outside the timed region.
            std::fs::write(&path, &base_bytes).unwrap();
            let previous = cached_projection(&path, None).unwrap();
            std::fs::write(&path, &appended_bytes).unwrap();
            let started = std::time::Instant::now();
            let projection = cached_projection(&path, Some(previous)).unwrap();
            let output = timeline_indexable_content(&projection.timeline);
            (started.elapsed().as_nanos(), output)
        });

        let rewritten = search_benchmark_timeline(turns, "bbbb");
        let rewritten_bytes = encode_timeline(&rewritten);
        assert_eq!(rewritten_bytes.len(), base_bytes.len());
        let rewritten_expected = timeline_indexable_content(&rewritten);
        report_search_samples(&format!("rewrite_fallback_{turns}"), &rewritten_expected, || {
            // Previous projection and same-size valid replacement are prepared before timing.
            std::fs::write(&path, &base_bytes).unwrap();
            let previous = cached_projection(&path, None).unwrap();
            std::fs::write(&path, &rewritten_bytes).unwrap();
            let started = std::time::Instant::now();
            let projection = cached_projection(&path, Some(previous)).unwrap();
            let output = timeline_indexable_content(&projection.timeline);
            (started.elapsed().as_nanos(), output)
        });
    }
}

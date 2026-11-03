//! Temporary hunk-patch benchmark source, kept outside the Cargo build.
//! For a pre-change baseline, use this complete fixture, old formatter, timing
//! helpers and baseline test. To repeat the current after comparison while the
//! shared test helpers remain in `diff.rs`, paste only `samples_ns`, `median`
//! and the comparison test into that test module; `patch_fixture` and
//! `old_generate_hunk_patch` already exist there. Baseline and after tests are
//! separate so baseline can run before the production refactor.

fn patch_fixture(line_count: usize, hunk_count: usize) -> (String, String, Vec<Hunk>) {
    let old_lines = (0..line_count)
        .map(|index| format!("{index:06} {}", "x".repeat(56)))
        .collect::<Vec<_>>();
    let mut new_lines = old_lines.clone();
    let mut hunks = Vec::with_capacity(hunk_count);
    for index in 0..hunk_count {
        let line = ((index + 1) * line_count / (hunk_count + 1)).min(line_count - 1);
        let old_text = format!("{}\n", old_lines[line]);
        let new_text = format!("{index:06} {}", "y".repeat(56));
        new_lines[line] = new_text.clone();
        hunks.push(Hunk {
            id: HunkId::new(),
            path: "probe.txt".into(),
            line_info: HunkLineInfo {
                old_start: line + 1,
                old_count: 1,
                new_start: line + 1,
                new_count: 1,
            },
            source: HunkSource::AgentEdit { prompt_index: 0 },
            old_text: Some(old_text),
            new_text: format!("{new_text}\n"),
            patch: None,
            created_at: chrono::Utc::now(),
            selected: false,
        });
    }
    (
        format!("{}\n", old_lines.join("\n")),
        format!("{}\n", new_lines.join("\n")),
        hunks,
    )
}

fn old_generate_hunk_patch(baseline: &str, current: &str, hunk: &Hunk) -> String {
    let old_lines: Vec<&str> = baseline.lines().collect();
    let new_lines: Vec<&str> = current.lines().collect();
    let mut output = String::new();
    let old_start_idx = hunk.line_info.old_start.saturating_sub(1);
    let new_start_idx = hunk.line_info.new_start.saturating_sub(1);
    let context_before_start = old_start_idx.saturating_sub(CONTEXT_LINES);
    let context_before_end = old_start_idx;
    let changes_end_new = new_start_idx + hunk.line_info.new_count;
    let context_after_start = changes_end_new;
    let context_after_end = (changes_end_new + CONTEXT_LINES).min(new_lines.len());
    let changes_end_old = old_start_idx + hunk.line_info.old_count;
    let context_after_start_old = changes_end_old;
    let context_after_end_old = (changes_end_old + CONTEXT_LINES).min(old_lines.len());
    let total_old_lines = (context_before_end - context_before_start)
        + hunk.line_info.old_count
        + (context_after_end_old - context_after_start_old);
    let total_new_lines = (context_before_end - context_before_start)
        + hunk.line_info.new_count
        + (context_after_end - context_after_start);
    let header_old_start = context_before_start + 1;
    let header_new_start = context_before_start + 1;
    let _ = writeln!(
        output,
        "@@ -{},{} +{},{} @@",
        header_old_start, total_old_lines, header_new_start, total_new_lines
    );
    for i in context_before_start..context_before_end {
        if let Some(line) = old_lines.get(i) {
            let _ = writeln!(output, " {line}");
        }
    }
    if let Some(old_text) = &hunk.old_text {
        for line in old_text.lines() {
            let _ = writeln!(output, "-{line}");
        }
    }
    for line in hunk.new_text.lines() {
        let _ = writeln!(output, "+{line}");
    }
    for i in context_after_start..context_after_end {
        if let Some(line) = new_lines.get(i) {
            let _ = writeln!(output, " {line}");
        }
    }
    output
}

fn samples_ns(mut run: impl FnMut() -> usize) -> Vec<u128> {
    for _ in 0..2 {
        std::hint::black_box(run());
    }
    (0..7)
        .map(|_| {
            let start = std::time::Instant::now();
            std::hint::black_box(run());
            start.elapsed().as_nanos()
        })
        .collect()
}

fn median(samples: &[u128]) -> u128 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

#[test]
#[ignore = "run on the pre-change source to capture the baseline"]
fn file_hunk_patch_baseline_microbenchmark() {
    for (label, lines) in [
        ("128", 128),
        ("512", 512),
        ("2048", 2_048),
        ("near_1MiB", 15_600),
    ] {
        for hunk_count in [1, 8] {
            let (baseline, current, hunks) = patch_fixture(lines, hunk_count);
            let raw = samples_ns(|| {
                hunks
                    .iter()
                    .map(|hunk| generate_hunk_patch(&baseline, &current, hunk).len())
                    .sum()
            });
            eprintln!(
                "T2 patch baseline lines={label} bytes={} hunks={hunk_count} raw_ns={raw:?} median_ns={}",
                baseline.len(),
                median(&raw)
            );
        }
    }
}

#[test]
#[ignore = "run after the refactor to compare the old reference with shared tables"]
fn file_hunk_patch_reuse_microbenchmark() {
    for (label, lines) in [
        ("128", 128),
        ("512", 512),
        ("2048", 2_048),
        ("near_1MiB", 15_600),
    ] {
        for hunk_count in [1, 8] {
            let (baseline, current, hunks) = patch_fixture(lines, hunk_count);
            let hunks = hunks.into_iter().map(Arc::new).collect::<Vec<_>>();
            let expected = hunks
                .iter()
                .map(|hunk| old_generate_hunk_patch(&baseline, &current, hunk))
                .collect::<Vec<_>>();
            assert_eq!(
                generate_hunk_patches(&baseline, &current, &hunks),
                expected,
                "check all patch bytes before timing"
            );

            let repeated = samples_ns(|| {
                hunks
                    .iter()
                    .map(|hunk| old_generate_hunk_patch(&baseline, &current, hunk).len())
                    .sum()
            });
            let reused = samples_ns(|| {
                generate_hunk_patches(&baseline, &current, &hunks)
                    .iter()
                    .map(String::len)
                    .sum()
            });
            eprintln!(
                "T2 lines={label} bytes={} hunks={hunk_count} repeated_raw_ns={repeated:?} repeated_median_ns={} reused_raw_ns={reused:?} reused_median_ns={}",
                baseline.len(),
                median(&repeated),
                median(&reused)
            );
        }
    }
}

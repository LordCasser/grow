//! Archived T1 first heap-only probe snapshot, preserved outside the Cargo
//! build. The persistent reference helpers and differential tests already
//! live in watcher.rs. To reproduce the caller shape, restore only
//! `timing_samples`, `median`, and the final ignored test below; do not paste
//! this snapshot over the existing helpers/tests. Reproducing the historical
//! raw algorithm also requires the heap-only selector noted below.
//!
//! The old selector is a typed semantic reference: depth is precomputed once
//! before stable sorting. It matches old selection order, but is not a
//! verbatim timing of the previous production `components().count()`
//! comparator. The `pure` raw label is historical: the outer timing wrapper
//! includes each owned-input `candidates.clone` and returned-path drop.
//! Filesystem samples include the full walk and result drop, but no OS watch
//! registration.
//! This snapshot records the first, heap-only selector. Reproducing its raw
//! values requires restoring that historical heap-only implementation too;
//! copying only the timing wrapper and ignored caller into today's tree calls
//! the final budget-aware selector and reproduces only the measurement shape,
//! not the first probe's algorithm. The final probe can be rerun against the
//! current production selector from `probe-budget-aware.rs`.

fn old_stable_sort_truncate(
    mut candidates: Vec<(usize, PathBuf)>,
    budget: usize,
) -> (Vec<PathBuf>, usize) {
    candidates.sort_by_key(|(depth, _)| *depth);
    let total = candidates.len();
    candidates.truncate(budget);
    (
        candidates.into_iter().map(|(_, path)| path).collect(),
        total,
    )
}

#[derive(Clone, Copy, Debug)]
enum CandidateDepths {
    Mixed,
    Equal,
    CyclicUp,
    CyclicDown,
}

fn synthetic_candidates(count: usize, shape: CandidateDepths) -> Vec<(usize, PathBuf)> {
    (0..count)
        .map(|ordinal| {
            let depth = match shape {
                CandidateDepths::Mixed => 1 + ordinal.wrapping_mul(17) % 6,
                CandidateDepths::Equal => 4,
                CandidateDepths::CyclicUp => 1 + ordinal % 6,
                CandidateDepths::CyclicDown => 6 - ordinal % 6,
            };
            let mut path = PathBuf::from("/synthetic-root");
            for component in 0..depth {
                path.push(format!("p{ordinal:05}-{component}"));
            }
            (path.components().count(), path)
        })
        .collect()
}

fn old_select_per_dir_watch_dirs(
    root: &Path,
    custom_ignore: &Option<GlobSet>,
    custom_include: &Option<GlobSet>,
    budget: usize,
) -> (Vec<PathBuf>, usize) {
    let dirs: Vec<_> = pruning_walker(root, custom_ignore, custom_include)
        .flatten()
        .filter(|entry| {
            entry.depth() > 0 && entry.file_type().is_some_and(|ft| ft.is_dir())
        })
        .map(|entry| entry.into_path())
        .collect();
    old_stable_sort_truncate(
        dirs.into_iter()
            .map(|path| (path.components().count(), path))
            .collect(),
        budget,
    )
}

#[test]
fn bounded_per_dir_selection_matches_stable_sort_boundaries() {
    for shape in [
        CandidateDepths::Mixed,
        CandidateDepths::Equal,
        CandidateDepths::CyclicUp,
        CandidateDepths::CyclicDown,
    ] {
        for count in [0, 1, 128, 512, 2_048, 12_000] {
            let candidates = synthetic_candidates(count, shape);
            let mut budgets = vec![0, 1, 8, 128, count, DEFAULT_MAX_WATCHES];
            budgets.sort_unstable();
            budgets.dedup();
            for budget in budgets {
                let expected = old_stable_sort_truncate(candidates.clone(), budget);
                let actual = select_shallowest_paths(candidates.clone(), budget);
                assert_eq!(
                    actual.1, count,
                    "full candidate count for {shape:?}, N={count}, B={budget}"
                );
                assert_eq!(
                    actual.0, expected.0,
                    "stable selection for {shape:?}, N={count}, B={budget}"
                );
                assert_eq!(actual.0.len(), count.min(budget));
            }
        }
    }
}

#[test]
fn bounded_per_dir_selection_matches_actual_12000_directory_walk() {
    let temp = TempDir::new().unwrap();
    for group in 0..120 {
        let group_path = temp.path().join(format!("g{group:03}"));
        fs::create_dir(&group_path).unwrap();
        for child in 0..99 {
            fs::create_dir(group_path.join(format!("d{child:03}"))).unwrap();
        }
    }

    for budget in [8, DEFAULT_MAX_WATCHES] {
        let expected = old_select_per_dir_watch_dirs(temp.path(), &None, &None, budget);
        let actual = select_per_dir_watch_dirs(temp.path(), &None, &None, budget);
        assert_eq!(actual.1, 12_000);
        assert_eq!(
            actual, expected,
            "actual filesystem selection at B={budget}"
        );
    }
}

fn timing_samples(mut run: impl FnMut() -> usize) -> Vec<u128> {
    for _ in 0..2 {
        std::hint::black_box(run());
    }
    (0..7)
        .map(|_| {
            let start = Instant::now();
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
#[ignore = "temporary T1 after probe; source retained in change/probe.rs"]
fn bounded_per_dir_selection_after_probe() {
    let mut raw = Vec::new();
    for shape in [CandidateDepths::Mixed, CandidateDepths::Equal] {
        for count in [0, 1, 128, 512, 2_048, 12_000] {
            let candidates = synthetic_candidates(count, shape);
            let mut budgets = vec![0, 1, 8, 128, count, DEFAULT_MAX_WATCHES];
            budgets.sort_unstable();
            budgets.dedup();
            for budget in budgets {
                let old = old_stable_sort_truncate(candidates.clone(), budget);
                let new = select_shallowest_paths(candidates.clone(), budget);
                assert_eq!(old, new, "{shape:?}, N={count}, B={budget}");

                let old_samples = timing_samples(|| {
                    let input = candidates.clone();
                    let started = Instant::now();
                    let (paths, total) = old_stable_sort_truncate(input, budget);
                    let _elapsed = started.elapsed();
                    std::hint::black_box(total + paths.len());
                    _elapsed.as_nanos() as usize
                });
                let new_samples = timing_samples(|| {
                    let input = candidates.clone();
                    let started = Instant::now();
                    let (paths, total) = select_shallowest_paths(input, budget);
                    let _elapsed = started.elapsed();
                    std::hint::black_box(total + paths.len());
                    _elapsed.as_nanos() as usize
                });
                let record = format!(
                    "pure shape={shape:?} n={count} budget={budget} retained={} total={} old_ns={old_samples:?} old_median_ns={} new_ns={new_samples:?} new_median_ns={}",
                    new.0.len(),
                    new.1,
                    median(&old_samples),
                    median(&new_samples)
                );
                eprintln!("T1 {record}");
                raw.push(record);
            }
        }
    }

    let temp = TempDir::new().unwrap();
    for group in 0..120 {
        let group_path = temp.path().join(format!("g{group:03}"));
        fs::create_dir(&group_path).unwrap();
        for child in 0..99 {
            fs::create_dir(group_path.join(format!("d{child:03}"))).unwrap();
        }
    }
    for budget in [8, DEFAULT_MAX_WATCHES] {
        let old_samples = timing_samples(|| {
            let started = Instant::now();
            let (paths, total) =
                old_select_per_dir_watch_dirs(temp.path(), &None, &None, budget);
            let elapsed = started.elapsed();
            std::hint::black_box(total + paths.len());
            elapsed.as_nanos() as usize
        });
        let new_samples = timing_samples(|| {
            let started = Instant::now();
            let (paths, total) =
                select_per_dir_watch_dirs(temp.path(), &None, &None, budget);
            let elapsed = started.elapsed();
            std::hint::black_box(total + paths.len());
            elapsed.as_nanos() as usize
        });
        let record = format!(
            "filesystem n=12000 budget={budget} old_ns={old_samples:?} old_median_ns={} new_ns={new_samples:?} new_median_ns={}",
            median(&old_samples),
            median(&new_samples)
        );
        eprintln!("T1 {record}");
        raw.push(record);
    }
    std::fs::write("/tmp/grow-fsnotify-t1-after.txt", raw.join("\n") + "\n")
        .expect("write T1 after samples");
}

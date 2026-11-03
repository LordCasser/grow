//! Temporary T1 baseline probe source, preserved outside the Cargo build.
//! It measures the old stable sort/truncate selector and the full current
//! pruning walker. The candidate matrix uses the same existing PathBuf key.

#[cfg(test)]
mod t1_before_probe {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::Instant;
    use tempfile::TempDir;

    fn samples_ns(mut run: impl FnMut() -> usize) -> Vec<u128> {
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

    fn old_stable_sort_truncate(candidates: &[PathBuf], budget: usize) -> (Vec<PathBuf>, usize) {
        let mut selected = candidates.to_vec();
        selected.sort_by_key(|path| path.components().count());
        let total = selected.len();
        selected.truncate(budget);
        (selected, total)
    }

    fn synthetic_candidates(count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|ordinal| {
                let depth = 1 + ordinal.wrapping_mul(17) % 6;
                let mut path = PathBuf::from("/synthetic-root");
                for component in 0..depth {
                    path.push(format!("d{ordinal:05}-{component}"));
                }
                path
            })
            .collect()
    }

    #[test]
    #[ignore = "temporary T1 old-selection baseline; probe source is kept in the change"]
    fn per_dir_selection_before_baseline() {
        let root = TempDir::new().unwrap();
        for group in 0..120 {
            let group_path = root.path().join(format!("g{group:03}"));
            fs::create_dir(&group_path).unwrap();
            for child in 0..99 {
                fs::create_dir(group_path.join(format!("d{child:03}"))).unwrap();
            }
        }
        let actual = samples_ns(|| {
            select_per_dir_watch_dirs(root.path(), &None, &None).len()
        });
        assert_eq!(select_per_dir_watch_dirs(root.path(), &None, &None).len(), 12_000);
        eprintln!(
            "T1 full-walker dirs=12000 raw_ns={actual:?} median_ns={}",
            median(&actual)
        );

        for count in [0, 1, 128, 512, 2_048, 12_000] {
            let candidates = synthetic_candidates(count);
            let mut budgets = vec![0, 1, 8, 128, count, DEFAULT_MAX_WATCHES];
            budgets.sort_unstable();
            budgets.dedup();
            for budget in budgets {
                let (expected, total) = old_stable_sort_truncate(&candidates, budget);
                assert_eq!(total, count);
                let samples = samples_ns(|| {
                    let (selected, observed_total) =
                        old_stable_sort_truncate(&candidates, budget);
                    std::hint::black_box(observed_total);
                    selected.len()
                });
                assert_eq!(expected.len(), count.min(budget));
                eprintln!(
                    "T1 pure-old-selection n={count} budget={budget} retained={} total={total} raw_ns={samples:?} median_ns={}",
                    expected.len(),
                    median(&samples)
                );
            }
        }
    }
}

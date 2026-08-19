//! Watcher startup-latency benchmark.
//!
//! All scenarios build ~12k total dirs so inotify-watch creation cost is
//! comparable across them:
//!
//! - `favorable` — most dirs live in a gitignored `target/` the new code skips.
//! - `fanout_w48_with_target` — 48 non-ignored top-level children PLUS a
//!   gitignored `target/`: a realistic moderate-width repo that fans out and
//!   skips the build dir (net win).
//! - `fanout_w64_no_ignored` — 64 non-ignored children, nothing ignored: the
//!   fan-out path's worst case (pure per-child `watch()` round-trip overhead,
//!   nothing to skip), bounding the cost at the threshold.
//! - `wide_w400` — 400 non-ignored children: above the threshold, so it
//!   exercises the recursive-root fallback (recursive-vs-recursive).
//! - `nested_ignored_js_shape` — node_modules-heavy tree where ~5/6 of the
//!   dirs are gitignored *below* the top level: per-dir mode (Linux default)
//!   prunes them; fan-out mode pays for them on emulated-recursion backends.
//!
//! Run with `cargo bench -p fsnotify --bench startup`. Medians land in
//! `target/criterion/watcher_startup/<scenario>/new/estimates.json`.
//! `GROW_FSNOTIFY_PER_DIR=0|1` pins the strategy for A/B runs.

use std::fs;
use std::path::Path;

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use fsnotify::{FsConfig, FsEventSource};
use tempfile::TempDir;

const BENCH_TOTAL_DIRS: usize = 12_000;
const SMOKE_TOTAL_DIRS: usize = 1_200;

fn is_benchmark_run() -> bool {
    std::env::args_os().any(|arg| arg == "--bench")
}

/// Criterion receives `--bench` only for a real `cargo bench` run. A target
/// launched without it by `cargo test --all-targets` is a correctness smoke
/// test, so preserve each tree's shape without spending the watcher's
/// production startup timeout on the full fixture.
fn total_dirs() -> usize {
    if is_benchmark_run() {
        BENCH_TOTAL_DIRS
    } else {
        SMOKE_TOTAL_DIRS
    }
}

/// Create `count` nested directories under `base`, grouped 100 per parent so
/// the tree has realistic fan-out and depth rather than one flat directory.
fn make_dirs(base: &Path, count: usize) {
    for i in 0..count {
        let dir = base.join(format!("g{}", i / 100)).join(format!("d{i}"));
        fs::create_dir_all(&dir).unwrap();
    }
}

/// Favorable: three watched subtrees plus a large gitignored `target/` holding
/// ~2/3 of the dirs. Kept at ~`TOTAL_DIRS` for comparability with the others.
fn build_favorable_tree() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    let total_dirs = total_dirs();
    let target = total_dirs * 2 / 3;
    let per = (total_dirs - target) / 3;
    make_dirs(&root.join("src"), per);
    make_dirs(&root.join("crates"), per);
    make_dirs(&root.join("tests"), per);
    make_dirs(&root.join("target"), target);
    temp
}

/// `width` non-ignored top-level children. When `with_target`, half the dirs
/// live in a gitignored `target/` (skipped by fan-out); otherwise nothing is
/// ignored.
fn build_wide_tree(width: usize, with_target: bool) -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    let total_dirs = total_dirs();
    let child_total = if with_target {
        total_dirs / 2
    } else {
        total_dirs
    };
    let per_child = child_total / width;
    for i in 0..width {
        make_dirs(&root.join(format!("pkg{i}")), per_child);
    }
    if with_target {
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        make_dirs(&root.join("target"), total_dirs / 2);
    }
    temp
}

/// JS-monorepo shape: ~1/6 of the dirs are sources across a few packages; the
/// rest live in `node_modules/` trees nested *below* the top level, which the
/// fan-out strategy's recursive child watches cannot skip (on inotify each of
/// those dirs still costs a watch descriptor) but per-dir mode prunes.
fn build_nested_ignored_tree() -> TempDir {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".gitignore"), "node_modules/\n").unwrap();
    let total_dirs = total_dirs();
    let src = total_dirs / 6;
    let ignored = total_dirs - 2 * src;
    for i in 0..4 {
        make_dirs(&root.join(format!("packages/pkg{i}/src")), src / 4);
        make_dirs(
            &root.join(format!("packages/pkg{i}/node_modules")),
            ignored / 8,
        );
    }
    make_dirs(&root.join("node_modules"), ignored / 2);
    make_dirs(&root.join("apps/web/src"), src);
    temp
}

fn bench_startup(c: &mut Criterion) {
    // `start` blocks on a std mpsc ready signal; the tokio loop is only spawned,
    // never awaited during timing, so a current-thread runtime suffices.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _guard = rt.enter();

    let mut group = c.benchmark_group("watcher_startup");
    group.sample_size(30);

    let mut run = |name: &str, tree: &TempDir| {
        let root = tree.path().to_path_buf();
        group.bench_function(name, |b| {
            // PerIteration drops each watcher (freeing its inotify watches)
            // before the next iteration and keeps the drop out of timing.
            b.iter_batched(
                || (),
                |()| FsEventSource::start(root.clone(), FsConfig::default()).expect("start"),
                BatchSize::PerIteration,
            );
        });
    };

    let favorable = build_favorable_tree();
    run("favorable", &favorable);

    let moderate = build_wide_tree(if is_benchmark_run() { 48 } else { 12 }, true);
    run("fanout_w48_with_target", &moderate);

    let worst = build_wide_tree(if is_benchmark_run() { 64 } else { 16 }, false);
    run("fanout_w64_no_ignored", &worst);

    // Keep the smoke width above the production fan-out cap so it still
    // exercises the recursive-root fallback.
    let wide = build_wide_tree(if is_benchmark_run() { 400 } else { 65 }, false);
    run("wide_w400", &wide);

    let nested = build_nested_ignored_tree();
    run("nested_ignored_js_shape", &nested);

    group.finish();
}

criterion_group!(benches, bench_startup);
criterion_main!(benches);

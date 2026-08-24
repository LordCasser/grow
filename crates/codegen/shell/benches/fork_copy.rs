//! Fork-path benchmark and profiling workbench.
//!
//! Synthesizes a session whose `updates.jsonl` matches a configurable target
//! size (production byte/line shape: user and agent chunks plus one bulky
//! trailing chunk), then measures `StorageAdapter::copy_session_data`, the
//! path that materializes the whole file and produced multi-GB RSS spikes on
//! large production sessions. Also the substrate for allocation/CPU profiling
//! (`cargo flamegraph --bench fork_copy`, dhat) and future peak-RSS bounds.
//!
//! Run: `cargo bench -p shell --bench fork_copy`
//! Size override: `FORK_BENCH_MB=64 cargo bench ...` (default 16 MB).

use std::hint::black_box;
use std::time::Duration;

use agent_client_protocol as acp;
use criterion::{
    BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group, criterion_main,
};
use shell::session::info::Info;
use shell::session::storage::{CopySessionOptions, JsonlStorageAdapter, StorageAdapter};
use shell::session::testkit::synth::synthesize_to_target_bytes;
use tempfile::TempDir;

fn bench_fork_copy(c: &mut Criterion) {
    let target_mb: u64 = std::env::var("FORK_BENCH_MB")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(16);
    let root = TempDir::new().expect("tempdir");
    let source = synthesize_to_target_bytes(root.path(), target_mb * 1024 * 1024);
    let adapter = JsonlStorageAdapter::with_root(root.path().to_path_buf());
    let updates_len = adapter
        .updates_snapshot_len(&source)
        .expect("updates ledger metadata");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("bench runtime");

    let mut group = c.benchmark_group("fork_copy");
    group
        .sampling_mode(SamplingMode::Flat)
        .sample_size(10)
        .measurement_time(Duration::from_secs(30))
        .throughput(Throughput::Bytes(updates_len));
    group.bench_function(
        BenchmarkId::new("copy_session_data", format!("{target_mb}MB")),
        |b| {
            let mut n = 0usize;
            b.iter(|| {
                n += 1;
                let target = Info {
                    id: acp::SessionId::new(format!("fork-bench-dst-{n}")),
                    cwd: "/bench/workspace-fork".to_string(),
                };
                let result = rt
                    .block_on(adapter.copy_session_data(
                        &source,
                        &target,
                        CopySessionOptions::default(),
                    ))
                    .expect("fork copy");
                // Keep each iteration's output dir from accumulating.
                rt.block_on(adapter.delete_session(&target))
                    .expect("delete benchmark target");
                black_box(result)
            });
        },
    );
    group.finish();
}

criterion_group!(benches, bench_fork_copy);
criterion_main!(benches);

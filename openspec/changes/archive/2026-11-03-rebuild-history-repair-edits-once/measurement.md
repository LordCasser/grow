# C2 repair-kernel measurement

The old implementations are test-only copies from the before snapshot at `/var/folders/gm/lzn3ylzx4lz7mx29lftngdz40000gn/T/grow-history-repair-baseline-d9filvce`; they are compared with the production functions in one process and same fixture. Fixture creation, `Vec` clone, output JSON serialization, and equality checks are outside each measured interval. Each case has 2 warmups and 7 timed samples per old/new implementation. The probe reports complete JSON equality and return-count equality on every warmup and sample.

Commands:

```sh
GROW_HOME=/tmp/grow-backlog-c2-home RUST_MIN_STACK=16777216 \
  cargo test --locked -p sampling-types --lib history_repair_kernel_timing_probe \
  -- --ignored --nocapture --test-threads=1

GROW_HOME=/tmp/grow-backlog-c2-home RUST_MIN_STACK=16777216 \
  cargo test --locked -p chat-state --lib quarantine_kernel_timing_probe \
  -- --ignored --nocapture --test-threads=1
```

The exact temporary probe source is preserved in [`probe-sampling-types.rs`](probe-sampling-types.rs) and [`probe-chat-state.rs`](probe-chat-state.rs). Raw medians and all 7 samples are in [`measurement-raw.txt`](measurement-raw.txt). These are kernel timings in debug test profile, not end-to-end history repair or Timeline replay timings.

| Kernel | N/K | Old median | New median | Observation |
|---|---:|---:|---:|---|
| Quarantine | 8,192 / 1 | 62.96 µs | 160.42 µs | One early edit pays for the full retain scan; slower on this sparse shape. |
| Quarantine | 8,192 / 64 | 807.58 µs | 404.42 µs | About 2.0× faster for distributed edits. |
| Quarantine | 8,192 / 512 (N/16) | 5.87 ms | 2.09 ms | About 2.8× faster. |
| Dangling insertion | 8,192 / 1 | 54.88 µs | 40.13 µs | Small absolute improvement on this fixture. |
| Dangling insertion | 8,192 / 64 | 595.38 µs | 196.71 µs | About 3.0× faster. |
| Dangling insertion | 8,192 / 512 (N/16) | 4.58 ms | 359.25 µs | About 12.8× faster. |
| Duplicate removal | 8,192 / 1 | 40.00 µs | 139.29 µs | One early removal is slower with the full retain pass. |
| Duplicate removal | 8,192 / 64 | 603.46 µs | 177.96 µs | About 3.4× faster. |
| Duplicate removal | 8,192 / 512 (N/16) | 4.57 ms | 433.83 µs | About 10.5× faster. |

The single-edit measurements expose sparse-workload regressions for quarantine and dedup; the probe does not justify claiming that every N/K shape gets faster. The intended benefit is reduced repeated suffix movement when many edits are distributed through longer histories. The no-edit health path remains a zero-count return and is measured separately as K=0.

Fixture-size note: the benchmark tag N is the filler vector length before duplicate triplets are inserted. At N=128/K=64, a dedup fixture is 192 items because 64 triplets require at least 192 slots. The other duplicate fixtures are N items; dangling/quarantine fixtures are N items. This is recorded so the smallest dense K64 data is not misread as an exact 128-item input.

The full mixed `repair_history_with_reason` test compares the final serialized conversation and all report fields against a pipeline composed from the old quarantine, dedup, and dangling algorithms plus unchanged normalize/strip functions. It is correctness evidence, not a timing measurement.

The quarantine function itself serializes historical evidence. That production serialization is inside the timed repair kernel; only the oracle/output comparison serialization is outside. The probe did not measure C2 heap/RSS. Quarantine retains K planned values and generated evidence; dangling merge temporarily owns both old and new item arrays, including a possible extra N-item allocation for sparse repairs. These are recorded trade-offs, not zero-allocation claims.

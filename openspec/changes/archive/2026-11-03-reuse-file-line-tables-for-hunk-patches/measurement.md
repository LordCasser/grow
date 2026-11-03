# Baseline and comparison

The pre-change baseline pointer is `/tmp/grow-backlog-file-line-tables-baseline-path.txt`; it contains `/tmp/grow-backlog-file-line-tables-before.txt`. The baseline used the unchanged production `generate_hunk_patch` path once per hunk. Full actor-boundary state tests were also run before the implementation and logged at `/tmp/grow-backlog-file-line-tables-actor-before.txt` (6 passed).

The post-change raw log is `/tmp/grow-backlog-file-line-tables-after.txt`. The temporary probe source, including fixture construction and the exact old formatter oracle, is preserved outside the build in `probe.rs`.

Commands used for both measurements:

```sh
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib diff::tests::file_hunk_patch_baseline_microbenchmark -- --ignored --exact --nocapture
GROW_HOME=/tmp/grow-home-hunk-tables RUST_MIN_STACK=16777216 cargo test --locked -p hunk-tracker --lib diff::tests::file_hunk_patch_reuse_microbenchmark -- --ignored --exact --nocapture
```

Each fixture's strings and hunk vector were built outside the timed region. The after probe first checked exact equality of every output patch between the copied old formatter called per hunk and the shared-table formatter. It then timed full patch formatting for the repeated old function versus the shared-table batch function on the same immutable fixture in the same test process. Each path used two warmups and seven raw samples; no setup or clone was timed. File sizes were 64 bytes per line: 8,192, 32,768, 131,072 and 998,400 bytes. Cases used one or eight replacement hunks.

| Lines / bytes | Hunks | Before repeated median (ns) | After old reference median (ns) | After shared tables median (ns) |
|---:|---:|---:|---:|---:|
| 128 / 8,192 | 1 | 13,583 | 14,208 | 13,958 |
| 128 / 8,192 | 8 | 104,959 | 107,042 | 20,666 |
| 512 / 32,768 | 1 | 45,708 | 45,625 | 45,667 |
| 512 / 32,768 | 8 | 382,209 | 368,333 | 53,500 |
| 2,048 / 131,072 | 1 | 183,292 | 181,000 | 178,792 |
| 2,048 / 131,072 | 8 | 1,439,750 | 1,449,583 | 186,042 |
| 15,600 / 998,400 | 1 | 1,370,750 | 1,318,375 | 1,323,958 |
| 15,600 / 998,400 | 8 | 10,269,416 | 10,762,208 | 1,348,500 |

Full raw samples, in nanoseconds, are retained in the before/after logs. The result shows near parity for one hunk and less repeated text scanning for multiple hunks. This is a formatter kernel measurement, not an end-to-end UI rendering measurement.

# Verification

- Built current `grow`: `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo build --locked --offline -p cli --bin grow` passed.
- Ran the ignored `resume_input_latency_128_512_turns` PTY probe with `PAGER_BINARY=$PWD/target/debug/grow`, `--locked --offline`, debug info and incremental compilation disabled: 1 passed. The probe enabled `GROW_INSTRUMENTATION=log` only in its isolated child process.

| Turns | PTY last history visible | Same-process Shell `load_session` | Shell replay | Pager render calls / sum | Pager diff sum | Writer I/O sum | Stable-key echo p95 |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 128 | 3,046 ms | 2,468 ms | 1,929 ms | 112 / 1,217 ms | 87 ms | 2.5 ms | 9.7 ms |
| 512 | 9,354 ms | 9,114 ms | 7,815 ms | 253 / 4,360 ms | 206 ms | 9.4 ms | 13.2 ms |

Instrumented 512-turn replay rendered 253 frames, with maximum individual render 57 ms; 128 turns rendered 112 frames, maximum 35.5 ms. The `pager.replay.end_batch` timer was 0.1/0.3 ms. Timer sums include some post-load key-echo frames, and frame rendering overlaps Shell tasks, so sums must not be added to form a wall-clock total. Compared with the separate no-Pager Shell benchmark (2.28 s at 512 turns), the same-process Shell load stretches to 9.11 s while Pager renders repeatedly during replay. This implicates replay-era frame work rather than terminal writer I/O or final batch indexing; the next focused change should cap replay paint cadence while retaining input fairness and progress visibility.

- `rustfmt --edition 2024` on touched Rust files, `openspec validate attribute-pager-resume-render-stages --strict --no-interactive`, and `git diff --check`: passed.

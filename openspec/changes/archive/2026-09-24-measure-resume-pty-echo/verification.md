# Verification

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 cargo build --locked --offline -p cli --bin grow`: passed, producing the current source revision of the binary used below.
- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 PAGER_BINARY=$PWD/target/debug/grow cargo test --locked --offline -p pager --test pty_e2e_persistence resume_input_latency_128_512_turns -- --ignored --nocapture --test-threads=1`: 1 passed. An initial `--offline` run updated the local lockfile for the new test-only `sampling-types` dependency.
- Synthetic 128-turn fixture: 7,680,607 replay-update bytes; resume-to-visible-history 3,042 ms; 20-key echo p95 7.5 ms, max 7.7 ms.
- Synthetic 512-turn fixture: 30,722,143 replay-update bytes; resume-to-visible-history 9,054 ms; 20-key echo p95 7.5 ms, max 7.6 ms.
- `rustfmt --edition 2024` on the new test module: passed.
- `openspec validate measure-resume-pty-echo --strict --no-interactive`: passed.

These are single runs on the freshly built local `grow` binary with the current Rust test harness, not a comparison of two revisions or a host-independent p95. A first run on the prior local binary gave 3,026 / 9,047 ms history visibility and 7.5 / 7.6 ms echo p95, providing a consistency check only. The fixture has ordinary historical text and synthetic ACP updates but does not cover dense real tool rows, multiple child sessions, resident reconnect/cursor, terminal image decoding, or a slow terminal. The 512-turn first-visible-history time rises with fixture size; the observed key echo remains below 100 ms once the prompt is stable. Keep the backlog item open pending the missing scenarios and phase attribution.

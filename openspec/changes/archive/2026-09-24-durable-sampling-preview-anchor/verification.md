# Verification

- `cargo check --locked --offline -p shell --tests` passed with incremental compilation and debug symbols disabled and two build jobs.
- 23 `session::persistence::durable_update_tests` passed. These cover one marker per attempt, physical pre/anchor/post/projection order, summary count, independent and anchor append failures, sticky pre-anchor failure, stale attempt barriers, discard, and exact projection retry.
- 15 `turn_pipeline_v2_tests`, 15 `response_projection::tests`, six sampling-attempt tests, actor marker delivery, and two projection fatal-boundary tests passed. Shell test threads used the project's required `RUST_MIN_STACK=16777216` setting; an initial default-stack run overflowed in a known stack-heavy turn test.
- Production read-only replay and cold writer repair after a Timeline admission with a payload-free anchor passed, including the crash gap before physical projection append.
- `rustfmt --edition 2024` and `git diff --check` passed. The persistence sender still has no aggregate capacity limit for independent updates; that separate end-to-end boundary remains in `openspec/backlog.md`.

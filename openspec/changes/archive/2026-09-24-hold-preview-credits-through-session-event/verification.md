# Verification

- `cargo test -p shell preview_credits_wait_for_actor_fence_and_close_on_lost_ack --lib` passed; the test holds the full credit budget while an earlier FIFO event is consumed, releases it only after the fence, and checks close-on-lost-ack.
- `cargo test -p shell sampling_attempt --lib` passed (6 tests).
- `cargo test -p sampler preview_fragment_budget_backpressures_and_preserves_order --lib` passed.
- `cargo test -p sampler preview_fragment_budget_wait_is_cancelable --lib` passed.
- Cargo used `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216`.
- `cargo fmt -p sampler -p shell` passed.

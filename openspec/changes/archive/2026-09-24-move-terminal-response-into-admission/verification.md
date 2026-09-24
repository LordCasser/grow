# Verification

- `response_items` and `native_continuation` have no downstream uses after `push_response_durably_with_identity`; assistant-message signals are counted before transferring their ownership and emitted after the same admission acknowledgement.
- `cargo check --locked --offline -p shell --tests` passed with incremental compilation and debug symbols disabled and two build jobs.
- `cargo test --locked --offline -p shell --lib response_projection::tests -- --test-threads=1` passed 15/15.
- `rustfmt --edition 2024`, strict OpenSpec validation, and `git diff --check` passed.

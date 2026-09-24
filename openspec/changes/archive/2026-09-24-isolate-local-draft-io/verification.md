# Verification

- Controlled store gates held load and write on the worker while the UI captured newer input; recovery, close, bind, ownership invalidation, retry, and quit races passed in the 31 focused local-draft tests.
- `cargo check --locked --offline -p pager --tests` passed with incremental compilation and debug symbols disabled and two build jobs.
- `cargo test --locked --offline -p pager --lib local_drafts::tests -- --test-threads=1` passed 31/31.
- `cargo build --locked --offline -p cli --bin grow` passed. The ignored PTY `local_draft_recovers_after_quit` then passed against that binary: normal exit returned zero, a draft file held the unsent text, and `--continue` recovered it without a model request.
- `rustfmt --edition 2024` and `git diff --check` passed for changed code. OpenSpec strict and archive checks are recorded by the archive command result.

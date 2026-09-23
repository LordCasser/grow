# Verification

## Ledger reproduction

Read-only inspection of target session `01a0bfba-e6b9-7a72-bb8a-3ceb78010cac` and compaction Sideband `01a0c49f-d46e-7632-87fc-736d6f541833` found:

- `selected_surface_ids`: 813;
- valid `Input::Consumed` coordinates: 12;
- valid `Notification::Consumed { input: Some(_) }` coordinates: 5;
- selected coordinates outside the frozen parent range `0..=10453`: 0.

The parent Timeline applies both consumed payloads to Surface. The former Sideband-local producer whitelist omitted both event kinds, so strict parent validation returned `InvalidSurfaceSelection`. The first failed peer inquiry occurred after the foreground turn had ended; the existing busy-foreground path and the augmented active-child regression both complete successfully, so foreground or background activity is not the admission cause.

## Commands

- `CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p chat-state`
  - 507 passed, 0 failed, 1 ignored.
- `RUST_MIN_STACK=33554432 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p shell --lib strict_reload_accepts_sideband_selected_consumed_input -- --nocapture`
  - 1 passed; strict JSONL session reload accepted a completed compaction Sideband selecting `Input::Consumed`.
- `RUST_MIN_STACK=33554432 CARGO_INCREMENTAL=0 CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked -p shell --lib delegated_inquiry_during_foreground_bypasses_peer_approval_without_injecting_input -- --nocapture`
  - 1 passed; target retained both an active foreground turn and an unterminated background child while receipt, tool-free answer and terminal audit completed.
- `rustfmt --edition 2024 crates/codegen/chat-state/src/sideband.rs crates/codegen/chat-state/src/timeline.rs crates/codegen/shell/src/session/actor/coordination.rs`
  - passed.
- `git diff --check -- <changed implementation, test and change paths>`
  - passed.
- `openspec validate --all --strict --no-interactive`
  - before archive: 22 passed, 0 failed;
  - after archive: 20 passed, 0 failed.
- `openspec validate --archived --no-interactive`
  - 352 passed, 0 failed, including `2026-09-22-fix-sideband-consumed-surface-validation`.
- `cargo clean`
  - final cleanup removed 8,109 files / 3.2 GiB; filesystem free space increased to 15 GiB.

## Environment notes

An initial unrestricted `cargo test -p shell` also compiled unrelated integration tests and exposed a pre-existing dirty-worktree non-exhaustive match for `SessionUpdate::ResponseReplayProjection`; the scoped `--lib` target avoids modifying that concurrent work. The first full-debug link exhausted the filesystem, so an intermediate `cargo clean` reclaimed 18.7 GiB and the tests were rerun with incremental compilation and test debuginfo disabled. The first coordination execution used the default test-thread stack and overflowed; rerunning the identical binary with `RUST_MIN_STACK=33554432` passed. The required final `cargo clean` ran only after all Rust verification completed.

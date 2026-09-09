# Verification

## Read-only session evidence

Session `01a081be-6168-7772-9e0c-dcc62a76b552` has three failed descendants at Timeline seq 16152, 16174 and 16196. Each fails at turn start with InvalidTurnIdentity before tool calls or token usage. Their Spawn records at seq 16145, 16167 and 16189 contain Goal `01a081e1-153d-77a1-b6c5-c15839761b90`, definition revision 1. No session files were modified.

The previous shared-window fix preserves the active Goal id but does not supply the missing turn revision. This regression uses actual handle_prompt admission with an empty child tracker and a shared active window, checking the committed TurnStarted pair. A mismatched inherited context must still fail without committing a turn.

Baseline actual handle_prompt regression failed with the same `turn_boundary_persistence_failed` / `InvalidTurnIdentity` error. Log: `/tmp/grow-delegated-owner-baseline.log`.

Fixed full Shell regression: `cargo test --locked --offline -p shell --lib -- --test-threads=2`: **3,762 passed, 0 failed, 3 ignored**, including actual child admission and mismatched-owner rejection. Log: `/tmp/grow-delegated-owner-fixed.log`. Build environment: CARGO_INCREMENTAL=0, dev/test debug=0, jobs=2, RUST_MIN_STACK=16777216 on macOS. No live provider calls were needed.

`git diff --check` and strict all-spec validation passed.

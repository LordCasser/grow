# Verification

- Failing Windows run: [34469589238](https://github.com/LordCasser/grow/actions/runs/34469589238), job 102846210811. 28 coordination cases passed and 9 failed: seven actor-fixture path failures, held-reader manifest replacement, and live sideband observation. Full log retained at `/tmp/grow-v2.1.6-windows-coordination-failed.log`.
- After correction, local full shell regression passed: 3,788 passed, 0 failed, 3 ignored. Command: `CARGO_INCREMENTAL=0 RUST_MIN_STACK=16777216 cargo test --locked -p shell --lib -- --test-threads=4`. Log: `/tmp/grow-v2.1.6-windows-fix-shell.log`.
- Existing manifest regression now additionally distinguishes the old reader's heartbeat from the new canonical heartbeat, while retaining token validation. The session observation regression additionally checks that observation does not populate the writer capability cache. All original scenarios remain present.
- `git diff --check` and OpenSpec strict validation passed (18 current items).
- Windows native, cross-platform communication and final Linux core execution are pending. This change stays unarchived until those required results are verified.

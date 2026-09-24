# Verification

- `cargo test -p sampler --lib`: 248 passed, 0 failed. The new producer test sends a 64 MiB + 1 byte text fragment and confirms a local lifecycle failure before queue admission. The new arithmetic test covers the exact limit and one byte beyond it without a second large allocation.
- `cargo fmt -p sampler` and `cargo check -p shell --tests` passed.
- Targeted and full strict OpenSpec validation passed; `git diff --check` passed.
- The Shell production sampling path passes a preview budget and an attempt evidence sink. This change also makes the public sampler handoff reject an oversized charged event if a future source bypasses the HTTP evidence cap.

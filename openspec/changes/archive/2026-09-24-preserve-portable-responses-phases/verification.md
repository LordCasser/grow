# Verification

- `cargo check --locked --offline -p sampling-types -p chat-state -p sampler`: passed.
- `cargo test --locked --offline -p sampling-types --lib -- --test-threads=1`: 300 passed.
- `cargo test --locked --offline -p chat-state --lib -- --test-threads=2`: 522 passed, 2 ignored.
- `cargo test --locked --offline -p sampling-types --lib responses_phases_survive_portable_projection_and_tool_pairing`: 1 passed after the final route-switch assertion.
- `cargo check --locked --offline --all-targets -p shell -p pager -p sampler`: passed after the independently scoped missing test match arm was repaired.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `openspec validate 2026-09-24-preserve-portable-responses-phases --strict --no-interactive`: passed before archive.

The focused regression serializes and restores the accepted Assistant item, switches to a portable Responses request, verifies both phase-bearing messages and one local call/result pair, then checks that Chat and Messages use the flat text. Separate tests cover malformed ranges, CWD rewriting, and compaction truncation.

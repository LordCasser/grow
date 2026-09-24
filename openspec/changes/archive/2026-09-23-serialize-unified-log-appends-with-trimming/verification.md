## Evidence

`write_lines` now delegates to `LogWriter::append_lines`, which first performs the existing maintenance check, then locks its persistent append descriptor for the complete write and unlocks on both success and error. `trim_file` already uses a nonblocking exclusive advisory lock on the same opened inode around its in-place rewrite/truncate. The original path identity and inode-preserving behavior remain unchanged.

One regression holds the trim lock, starts a production writer append, rewrites/truncates the inode, then releases the lock. It verifies the append waited and the final file contains the retained tail followed by the new line. A second regression holds the writer descriptor lock and verifies `trim_file` yields without modifying the file. The backlog now tracks only the separate unbounded single record and slow synchronous I/O edges.

## Validation

- `cargo test --locked --offline -p diagnostics --lib append_waits_for_trim_and_survives_truncate -j 1 -- --test-threads=1` — 1/1 passed.
- `trim_yields_while_writer_owns_inode_lock` rebuilt and passed, then the full diagnostics test binary passed 59/59.
- `rustfmt --edition 2024 --check crates/codegen/diagnostics/src/unified_log.rs`, `git diff --check`, and `openspec validate serialize-unified-log-appends-with-trimming --strict --no-interactive` passed.
- `openspec validate --all --strict --no-interactive` — 18/18 passed before archive.
- `openspec validate --all --strict --no-interactive` — 17/17 passed after archive.
- `openspec validate --archived --no-interactive` — 385/385 passed after archive, including the parallel docs-only backlog cleanup.

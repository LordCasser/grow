## Evidence

`emit` and every Pager entry in `ingest_client_entries` now use `encode_entry_line`. The encoder streams into a writer that checks each serialized chunk against a 65,535-byte JSON budget before extending its buffer, then appends one LF. The overflow path discards that buffer and encodes a compact replacement carrying the producer source, level and PID plus a fresh timestamp, fixed omission reason and 65,536-byte limit. It drops the original session, message, context and client version. Thus no prefix of the rejected encoding reaches the append path.

The exact-limit regression verifies an unchanged 65,536-byte complete line. The oversized regression supplies multi-megabyte message and version fields, then verifies the replacement fits the limit, has exactly one LF, deserializes as a complete `LogEntry`, identifies the omission and does not retain the oversized message.

The original backlog entry was narrowed after archive to synchronous lock/write latency only. The bounded encoder closes its single-record size and newline-boundary portion; it does not impose a file-wide hard quota or a deadline on blocking file I/O.

## Validation

- `cargo test --locked --offline -p diagnostics --lib unified_log::tests -j 1 -- --test-threads=1` — 25 passed.
- `cargo test --locked --offline -p diagnostics --lib -j 1 -- --test-threads=1` — 61 passed.
- `rustfmt --edition 2024 --check crates/codegen/diagnostics/src/unified_log.rs` and `git diff --check` passed after formatting.
- `openspec validate bound-unified-log-record-size --strict --no-interactive` passed.
- `openspec validate --all --strict --no-interactive` — 18/18 passed before archive.
- `openspec validate --all --strict --no-interactive` — 17/17 passed after archive.
- `openspec validate --archived --no-interactive` — 386/386 passed, including this archive.

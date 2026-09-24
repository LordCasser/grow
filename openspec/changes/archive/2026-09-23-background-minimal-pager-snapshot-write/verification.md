# Verification

## Measurement and boundary decision

Environment: macOS 26.6.2, Apple M4 Pro (arm64), Cargo test profile (`unoptimized + debuginfo`), serial samples, temporary files in `std::env::temp_dir()`. A temporary ignored test constructed one assistant message from repeated ASCII `x` bytes, timed `render_blocks_to_markdown`, then timed the production `write_pager_transcript` call on the rendered body. Each size was sampled three times; report medians and the sample range.

| Input | Markdown output | Render median (range) | Snapshot write median (range) |
| ---: | ---: | ---: | ---: |
| 1 MiB | 1,048,590 bytes | 0.046 ms (0.045–0.374) | 0.739 ms (0.242–1.429) |
| 10 MiB | 10,485,774 bytes | 0.368 ms (0.321–0.379) | 2.622 ms (2.445–4.457) |
| 100 MiB | 104,857,614 bytes | 3.541 ms (3.340–3.751) | 42.191 ms (24.203–142.095) |

Minimal's full-fidelity renderer runs inside the UI draw hook in slices capped at 8 ms. Before this change, the final slice synchronously called the same snapshot writer before returning from the hook. The 100 MiB write therefore consumed 24–142 ms in that frame, exceeding the existing per-frame rendering budget. The input-latency p95 ≤ 100 ms in the 2026-09-12 architecture review is explicitly a proposed UX target, not an archived SLA; this change relies on the existing 8 ms Minimal slice budget instead.

Full/TUI runs compact Markdown rendering and snapshot writing synchronously in dispatch. Its measured Markdown conversion stayed below 4 ms; 100 MiB snapshot writing had a 142 ms maximum sample. No archived Full/TUI overall response-time boundary was found, so this change does not alter that path or promote the proposed 100 ms target into a contract.

## Change verification

- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib minimal_snapshot_completion -- --nocapture --test-threads=1`: 2 passed. Covers child owner delivery, superseded result cleanup, root binding replacement, and owner-bound error notice.
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib blocked_minimal_snapshot_write_keeps_runtime_responsive -- --nocapture --test-threads=1`: 1 passed. An injected writer waits on a release channel while the current-thread Tokio runtime receives a queued input event; the complete private ANSI file is then checked, including Unix 0600.
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib draw_queued_minimal_snapshot_starts_without_another_event -- --nocapture --test-threads=1`: 1 passed. A loop-tail Effect created by the draw hook starts the writer immediately without waiting for an additional input/ACP event.
- `CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager-minimal --lib transcript -- --nocapture --test-threads=1`: 5 passed. Compiles the Minimal call site and preserves transcript rendering regressions.
- The temporary scale-measurement test was removed after collecting results; no benchmark harness or persistent test-only instrumentation was added.

## Limits

The controlled writer test checks current-thread Tokio scheduling and result ownership, not physical terminal echo latency in a PTY. The scale samples use a synthetic single assistant message and the local temporary filesystem, not a real long session or slow disk. Full/TUI was not exercised under a blocked filesystem because no archived response bound applies to that path. Measurements are local samples, not latency guarantees; the architectural guarantee is that Minimal snapshot file I/O now runs in a blocking worker and stale owner/request results are dropped.

- After the final loop-tail change, `openspec validate --all --strict --no-interactive`: 19 passed, 0 failed; `openspec validate --archived --strict --no-interactive`: 394 passed, 0 failed.

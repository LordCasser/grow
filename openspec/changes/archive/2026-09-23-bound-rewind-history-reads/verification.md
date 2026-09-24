# Verification

The active pinned-source reader now rejects a physical line above 64 MiB, a scan above 256 MiB, and a scan above 50,000 records. Metadata and full-history reads use the same bounded reader. The metadata visitor checks both snapshot maps, relative paths, optional string content, timestamps and unknown fields while retaining only counts. Failed metadata returns `io::Error` through the session command and `grow/rewind/points`; the existing Pager failure branch restores the draft and reports the error. A failed full load keeps live points and the pinned source for retry.

`scan_pinned_source` moves the owned source guard into `spawn_blocking`. The awaiting future can be cancelled without letting another seek race its still-running worker. The guard remains held across the full-load merge or metadata/live snapshot combination after a successful scan. The deterministic cancellation test gates a worker, aborts its caller, confirms independent executor progress, verifies the source lock remains unavailable, and then verifies it remains retryable after worker release.

Evidence:

- `cargo check --locked --offline -p workspace -p shell`: passed after the initial implementation; the subsequent Rust test builds recompiled the final shared reader and Shell result types.
- `cargo test --locked --offline -p workspace --lib session::file_state::tests`: 35 passed, including byte/count limits, nested damage, retry and cancellation ownership. A focused rerun of the cancellation test passed after the last assertion was added.
- `cargo test --locked --offline -p shell --lib rewind_points_`: 4 passed, including ACP failure propagation and load-barrier routing.
- `cargo test --locked --offline -p shell --lib rewind_file_counts_maps_snapshot_metadata`: passed.
- `cargo test --locked --offline -p pager --lib failed_rewind_metadata_restores_draft_and_reports_error`: passed; the current interaction closes, restores its draft and shows the metadata failure toast.
- `cargo fmt --all`, `git diff --check`, change strict validation and prearchive `openspec validate --all --strict --no-interactive`: passed (16/16).
- After archive, strict active/current validation passed 15/15, archived validation passed 431/431, and `git diff --check` passed.

The scan bounds apply to pinned history, not newly captured live snapshots. Decoded object overhead can exceed raw JSON bytes, and a blocked filesystem syscall can outlive its caller; the worker retains ownership so that a retry cannot race it. No actual slow filesystem is used in the test; a deterministic gated worker exercises the same ownership seam.

## 1. Strict search discovery

- [x] 1.1 Add a search-specific `StorageAdapter` enumeration contract and implement JSONL incomplete-candidate failure while preserving ordinary `list_sessions` behavior; verify a missing or malformed summary returns an error through the new path and remains skipped by the ordinary path.

## 2. Bootstrap failure boundary

- [x] 2.1 Clear the completed marker transactionally under the current claim before reindexing; verify marker deletion fails closed when another token owns the claim.
- [x] 2.2 Propagate per-session Timeline, timeout, index-write and task-panic failures from the JoinSet so incomplete passes skip pruning and marker publication; verify existing oversized-session title-only success still counts as a completed task.

## 3. Regression and contract validation

- [x] 3.1 Add real temporary-root tests for missing/malformed summary preservation and Timeline failure followed by repair plus `RecheckBootstrap`; verify preserved prior rows, absent marker before repair, and queryable recovered row after recheck.
- [x] 3.2 Run the focused `shell` search bootstrap tests, `rustfmt --check` on changed Rust files, and `openspec validate retain-session-search-index-on-incomplete-bootstrap --strict --no-interactive`; record each result in `verification.md` before checking tasks and archiving.
- [x] 3.3 Run `openspec validate --all --strict --no-interactive` and `openspec validate --archived --no-interactive` after archive; record results in the change verification record.

## Verification

- `cargo test --locked -p pager --lib local_drafts::tests::quarantine -- --nocapture` — passed: 3 quarantine retention tests. The linker emitted a non-fatal compact-unwind warning for the pager test binary.
- `openspec validate --all --strict --no-interactive` — passed: 17 items, 0 failures.
- After the shared build lock cleared, `RUST_MIN_STACK=16777216 cargo test -p pager --lib local_drafts::tests -- --test-threads=1 --quiet` passed all 21 tests.
- `openspec validate --all --strict --no-interactive` after archive — passed: 16 items, 0 failures.
- `openspec validate --archived --strict --no-interactive` — passed: 462 archived changes, 0 failures.

The regression coverage checks the 64-entry bound, the 16 MiB byte bound, age ordering, filename tie-breaking, and that symlink targets are not followed. Path replacement races and slow filesystem deadlines remain outside this change.

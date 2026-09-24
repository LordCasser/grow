## Audit result

The backlog's wording is correct only for the duplicate-row merge helper, not for the final append result. `merge_coordination_rows_from_tail` at `crates/codegen/pager/src/scrollback/state/coordination.rs` obtains an existing row through `coordination_entry_id(...)?`; when none exists, `filter_map` skips the merge tuple and leaves the tail entry untouched.

`ScrollbackState::append_entries_from` in `crates/codegen/pager/src/scrollback/state/mod.rs` then:

- extends `self.entries` with the remaining `tail.entries`, preserving their `EntryId`s;
- extends `self.running` with `tail.running`;
- merges `tail.minimal_commit` into the destination commit set.

Therefore an incoming tail-only inquiry survives append with its row identity, running state, and committed state. Existing production callers are root and child session reload resolution in `app/agent_view/session.rs` (lines 554 and 801).

No runtime code or test was changed. Per the coordinating task's disk-space constraint, the proposed Rust regression was removed before any Cargo build; this conclusion is based on the direct append/merge implementation evidence above. The remaining production `expect`, notification ordering/missing identity, replay and passive lifecycle concerns stay open in the backlog.

## Validation

- `openspec validate audit-tail-only-coordination-append --strict --no-interactive` — passed.
- `openspec validate --all --strict --no-interactive` — passed, 20/20 items.
- `git diff --check` — passed.
- `openspec validate --archived --no-interactive` — passed, 413/413 archived changes.

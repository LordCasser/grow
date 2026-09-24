## Why

The backlog says `merge_coordination_rows_from_tail` ignores a tail-only coordination inquiry. The helper does skip identity merging when the original transcript has no matching row, but `append_entries_from` subsequently appends the remaining tail entries. A focused regression should make the resulting user-visible row and its live/commit state explicit.

## What Changes

- Record the implementation evidence for a tail-only inquiry appended into an empty original scrollback.
- Clarify that entry identity, running state, and already-committed state are carried by the append path.
- Narrow the backlog wording to distinguish skipped identity merging from preserved append behavior.

## Capabilities

No user-visible behavior changes. `.openspec.yaml` sets `skip_specs: true`; this is a code-evidence audit and regression test for existing behavior, so it adds no contract delta.

## Impact

- Backlog: only the tail-only inquiry clause in the coordination/sideband identity item.
- No production or test code changes.
- The production `expect` and passive-row lifecycle concerns remain open and out of scope.

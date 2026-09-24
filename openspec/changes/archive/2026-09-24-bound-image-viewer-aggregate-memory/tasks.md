## Implementation

- [x] Add process-wide viewer byte admission covering source copy, conversion workspace, and retained encoded buffers.
- [x] Carry the reservation with loaded data into viewer ownership; release on failure, close/reopen, and stale result discard.
- [x] Add deterministic aggregate-budget and owner-lifecycle tests, plus a representative peak measurement.
- [x] Update developer architecture documentation and narrow/remove the backlog entry according to verified scope.

## Verification

- [x] Run focused tests, formatting, diff and strict OpenSpec checks; record validation, archive, and revalidate.

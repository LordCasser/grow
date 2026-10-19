# Acceptance

- [ ] Duplicate, mismatched, and unmatched interaction events fail Timeline validation; an exact open/resolve pair replays with its original attempt identity.
- [ ] Question and Plan approval requests persist Opened before ACP dispatch and persist the correct resolution for answer, cancellation, timeout, malformed response, and transport failure.
- [ ] Permission requests that are auto-granted or locally denied do not emit human interaction events; a real ACP permission request has one exact open/resolve interval.
- [ ] Suspension closes a live interaction attempt at the suspension boundary; restoration records a fresh attempt, excluding offline time from user wait duration.
- [ ] A validated same-operation TreeSuspended/ExecutionReleased pair subtracts offline time from open activity, while Resumed without release remains offline.
- [ ] Unrelated overlapping work remains protected, and missing/reversed lifecycle evidence remains unknown rather than being guessed.
- [ ] Compression applies the strict 30-second threshold to final fragments after splitting; short offline/idle pieces are not stretched, and source-to-playback mapping remains monotone.
- [ ] Current-format histories without typed interaction facts remain unknown and are not backfilled; unsupported older Timeline formats follow the schema-version rejection policy.

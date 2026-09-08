# Verification

## Reproduction
Before the production change, minimal_transcript_waits_for_owner_reload failed at the assertion that take_minimal_transcript must return None while the owner scrollback is stashed. The old function returned the build, allowing the unchanged pump to skip missing IDs.

## Implementation evidence
The sole production begin_session_reload call is in the event-loop reconnect load-plan loop. Immediately after that loop, restart_minimal_transcript_after_reload receives exactly the selected reload agent IDs, invalidates matching pending build IDs/output, and marks restart. This executes synchronously before awaiting any reload response, covering reload completion between frames. take_minimal_transcript waits while the owner SessionReload exists and refreshes IDs from the final state afterward. New requests during reload also carry restart intent, even if staging has no entries. Removed-agent behavior remains delegated to the existing pump.

## Tests
- pager --lib minimal_transcript: 4 passed. The restart matrix covers full replay success, cursor success, failure rollback, each with or without a pump during reload (6 combinations). It starts with a partially rendered prefix and verifies all final IDs, next=0, and empty output. Additional tests cover waiting, new requests during reload, and unrelated agent/view changes preserving the prefix. The existing UI-clock test still passes.
- This checks the exact build that the real pump consumes; no claims of a real network reconnect or PTY session.
- OpenSpec all strict before archive: 16 passed. git diff --check passed.
- Existing macOS compact-unwind-size linker warning remains.

## Limits
The lifecycle hook must accompany any future production reload entry point. This change does not restart already generated immutable files, alter initial history-loading behavior, or snapshot all message contents at request time. Incremental rendering and same-session rewind semantics otherwise remain unchanged. No CLI relink is claimed.

- pager-minimal --lib full_view: 8 passed, 0 failed. Combined relevant checks total 12 tests. target 13 GiB, available disk 64 GiB.

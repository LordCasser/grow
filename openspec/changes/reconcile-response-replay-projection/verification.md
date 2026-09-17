# Verification

## Completed checks

- `cargo test --locked -p chat-state admitted_response_view -- --test-threads=1` — passed, 4 tests. Covers healthy/identity-bearing ownership, quarantine integrity repair, compaction preservation, rewind retaining an earlier response, rewind removing later responses, compaction-prefix rewind provenance, and legacy identity-less exclusion.
- `cargo test --locked -p shell --lib response_projection -- --test-threads=1` — passed, 4 tests. Covers deterministic reasoning/text projection without fabricated event IDs, quarantine Discarded with zero raw preview, exact/conflicting record comparison, and the typed fatal `response-projection` marker.
- `cargo check --locked -p shell -p chat-state` — passed.
- Changed-file `rustfmt` — completed. Rustfmt recursively touched sibling modules; those unrelated formatting-only changes were restored before handoff.
- `git diff --check` — passed.
- `openspec validate reconcile-response-replay-projection --strict --no-interactive` — passed.

## Implemented production paths

- Timeline-owned selected-branch response provenance, independent of display-leaf Surface IDs.
- Event-FIFO live projection barrier and typed fatal boundary.
- Exact eventId-keyed append for independent ACP rows; missing IDs and payload conflicts fail closed.
- Shared projection scan/validation/synthesis core used by typed/direct replay, streaming child replay, raw MvpAgent replay, ordinary typed load, and cold replacement-writer repair.
- Missing `updates.jsonl` is treated as an empty cache and can synthesize a safe trailing response in memory.
- Projection records remain id-less; cursor replay consequently uses the existing full-replay safety fallback.

## Known verification limits

The following requested fault/end-to-end cases were not covered by dedicated automated tests in this pass, so OpenSpec tasks remain unchecked:

- injected failure at independent interleaving row N and at projection append, including committed-error/ACK-loss retries;
- full cold/resident raw replay and cursor production harness assertions;
- child/export tests proving zero provider/tool dispatch during reconciliation;
- an end-to-end completion-requirement harness proving no provider recovery. Production code checks the typed fatal marker before completion recovery, and the marker itself is unit-tested, but this was not exercised with a provider counter;
- package-wide tests and all-OpenSpec validation were intentionally left to the parent;
- `cargo clean` was not run.

## Evidence

Pager's `FocusTracker` owns the away-period UUID; dispatch copies it into the existing `SendRecap` effect and ACP `grow/recap` params. Shell rejects an automatic request without this ID, carries it through the session command, and echoes it on a successful `SessionRecap`. Pager compares live automatic results to its current period before appending a block or marking the session shown. Historical replay bypasses this live gate and remains display-only; manual requests carry no period ID.

The regression injects an A result after B has begun, including a missing-ID live result. Neither appears in scrollback or consumes B's eligibility; a B result succeeds. Additional tests cover focus-return identity retention, production request serialization, Shell command acceptance/validation, notification wire roundtrip, replay and manual behavior.

## Validation

- `openspec validate bind-auto-recap-to-away-period --strict --no-interactive` — passed.
- Shell recap tests from the newly built test binary: `RUST_MIN_STACK=33554432 ... recap_ --test-threads=1` — 33/33 passed. The first run with the default test-thread stack overflowed in an existing deep recap test; the larger test-thread stack rerun passed.
- Pager recap tests from the newly built test binary: `RUST_MIN_STACK=33554432 ... recap_ --test-threads=1` — 47/47 passed. After moving the ID check before backoff bookkeeping, `automatic_recap_dispatch_carries_the_current_away_period` was rebuilt and passed again.
- `rustfmt --edition 2024 --check` passed for the core recap request/notification types, FocusTracker, and dispatch; `git diff --check` passed. Large pre-existing test files have unrelated formatting drift, so this change keeps its edits local.
- `openspec validate --all --strict --no-interactive` — 19/19 passed before archive.
- `openspec validate --all --strict --no-interactive` — 18/18 passed after archive.
- `openspec validate --archived --no-interactive` — 382/382 passed after the archive task was checked.

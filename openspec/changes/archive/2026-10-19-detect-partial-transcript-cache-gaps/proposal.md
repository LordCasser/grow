# Detect partial offline transcript cache gaps

## Why

Offline transcript projection currently rejects a wholly missing display ledger, but a partially truncated ledger can omit committed user input echoes and still appear readable. Persisted Workflow and Control projections can also be missing without a matching Timeline-to-row identity. Reconstructing a display update without an exact identity mapping would invent history.

## What Changes

- Require complete cached ACP user echoes for current-branch committed inputs, using exact prompt identity and a durable ordered-block fingerprint to detect missing or truncated chunks.
- Persist the original prompt identity on `TurnStarted` and require an exact cached `TurnCompleted` for eligible durably ended turns.
- Fail with a specific invalid-data error when a matching display row is absent or incomplete; do not infer identity from timestamps or neighboring updates.
- Keep rewind filtering ahead of the check so discarded branch inputs do not create false gaps.
- Record a bounded Timeline expectation for each durable Workflow, Goal, Control, or CurrentMode display projection, binding its source identity and captured revision to the exact persisted row identity and payload hash before the row is published.
- Reject a captured transcript read when an expected Workflow, Goal, Control, or CurrentMode row is absent or its payload does not match; leave ephemeral/coalesced projections without durable expectations.

## Scope

This change detects missing or altered user echo chunks, missing eligible TurnCompleted rows, and missing or altered durable Workflow/Control projection rows. It does not rebuild display rows or synthesize terminal payloads. Current-format histories that lack required identity facts remain incomplete rather than being repaired from timestamps. Unsupported older Timeline formats follow the schema-version rejection policy.

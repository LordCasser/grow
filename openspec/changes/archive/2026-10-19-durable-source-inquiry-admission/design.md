## Context

The logical inquiry identity is `(source_session_id, inquiry_id)`. A source call sends `RecordCoordinationInquiry` to its SessionActor, which owns canonical Timeline writes and UI notice projection. This serialized command is the existing admission boundary; process-local runtime caches are not durable owners.

## Decision

For `OutgoingStarted`, the SessionActor reads its canonical Timeline before writing. If the same inquiry ID is already bound to the same target and exact question, it returns accepted without another append or projection. If the ID is bound to another target or question, it returns a typed `Conflict` without mutation. If no binding exists, it durably appends and projects the first startup fact through the existing path.

This read-before-write is serialized by the actor command loop, so concurrent retries cannot both observe absence. If a caller times out after the durable append, a later command observes that canonical fact and returns the same admission result. Question text is compared only as part of the payload bound to the explicit inquiry ID; it is never a deduplication key.

## Validation

Exercise first admission, exact retry, conflicting target, conflicting question, and a caller retry after the canonical fact is already present. Assert Timeline and UI projection each contain at most one startup fact, and conflicts leave the Timeline unchanged. Do not run Cargo in this work slice; root will perform unified Rust validation.

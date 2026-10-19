## 1. Durable source admission

- [x] Add the local-coordination delta for serialized, idempotent source startup admission.
- [x] Make the SessionActor check canonical `OutgoingStarted` facts before appending or projecting.
- [x] Preserve typed `Conflict` through the source command response.
- [x] Add tests for first admission, exact retry, conflicting target/question, and retry after durable acceptance.

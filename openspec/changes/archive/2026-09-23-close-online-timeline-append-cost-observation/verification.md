# Verification

Closed the backlog observation after reviewing the current Timeline implementation and the archived focused benchmark. The benchmark reports unoptimized test-profile medians: with 100 retained completed request identities, prepare+accept took about 21 μs and requested 14,116 B; with 1,000 identities, about 190 μs and 131,604 B. Increasing prior message/event/Surface history from 0 to 10,000 did not change the measured cost. A rejected append preserved an equal full Timeline snapshot.

The measurement isolates `Timeline::prepare/accept`; it excludes actor persistence and scheduling and is not a release or end-to-end measurement. No product latency budget or representative production lifecycle-size distribution is available to show that the measured cost violates an expected boundary. This establishes lifecycle-cardinality scaling, not an actionable user-visible defect, so the conditional observation is closed without a runtime optimization.

The benchmark method, complete result matrix, atomicity assertion, and explicit limits remain in the [archived online append measurement](../2026-09-23-measure-online-timeline-append/verification.md). The earlier [long Timeline replay optimization](../2026-09-10-accelerate-long-timeline-replay/verification.md) concerns recovery's repeated full-fold replay cost and is separate from this online append observation.

No Cargo commands were run. OpenSpec validation and archive checks are recorded below.

# Design

Treat the backlog as a list of concrete unresolved work, not a catalogue of synchronous calls or possible future guarantees. For each entry, preserve only what current code or archived evidence establishes.

- Timeline live append has a directly visible lifecycle clone and prior replay measurements establish that accumulated-state copies can dominate large histories. Keep a live-append benchmark as the evidence gate; preserve transactional failure behavior.
- Skill discovery has exact synchronous call sites alongside a separately bounded reload worker. Measure only those call sites and the existing worker shutdown ownership boundary. A slow filesystem delay is an input to the experiment, not a claim about production impact.
- Explicit copy/export file I/O already runs in a bounded queue. No evidence establishes a remaining responsiveness or memory failure, so remove that speculative entry.
- `/transcript` performs synchronous markdown rendering and temporary snapshot writing. Keep one narrowly scoped measurement to decide whether the call crosses an existing response expectation. Treat crash leftovers as a separate product policy.
- Scroll logging is opt-in and synchronous. Its performance impact has not been observed and no latency contract applies. Explicit paths, however, are opened with truncation and have no inter-process ownership mechanism; retain that distinct correctness question.

This change does not introduce a new response-time SLA. Where no existing deadline or response expectation applies, the measurement should report the observed latency and result in removing the backlog item rather than inventing a product guarantee.

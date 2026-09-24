# Change: Clarify performance backlog boundaries

## Why

Several backlog entries describe synchronous code paths but have no reproduced user impact or response-time contract. Keeping them phrased as defects conflates known implementation details with measured failures and speculative product guarantees.

## What Changes

- Remove the copy/export responsiveness entry because it records only unmeasured latency and memory assumptions.
- Keep long Timeline append copying as a measurable implementation hotspot, distinguishing the existing replay benchmark from live append cost.
- Narrow skill discovery and external-pager entries to exact call sites, a minimal measurement, and a decision rule that does not assume a stall occurred.
- Keep scroll-recorder explicit-path multi-process ownership as a correctness question; remove unmeasured slow-disk latency and future retention guarantees from that entry.

## Impact

This is a docs-only backlog clarification. It changes no product behavior or accepted specification, so `skip_specs: true` is used. Only the five named backlog entries and this change's records are in scope.

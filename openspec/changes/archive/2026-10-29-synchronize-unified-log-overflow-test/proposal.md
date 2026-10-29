## Why

The unified-log overflow test assumes the first queued record has already consumed the dropped-record counter once the worker removes it from the bounded channel. The worker swaps that counter before acquiring the writer mutex, so an overflow can be counted either before or after the first append observes it. CI exposed the test's unstable fixed-position assertion.

## What Changes

- Synchronize the blocked-writer fixture at record-processing boundaries instead of assuming queue dequeue means the counter was observed.
- Cover overflow accumulated before the first append and overflow accumulated after a committed first record.
- Preserve nonblocking producer, complete record order, and exactly-one-drop assertions.

## Scope

This is test-only synchronization. Production queue capacity, counter ordering, record ordering, and drop semantics do not change. `.openspec.yaml` uses `skip_specs: true` because no behavior contract changes.

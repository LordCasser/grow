## Why

The existing ignored resume latency probe measures key echo only after history replay completes. That does not exercise the user-input path during the cold replay window under a slow terminal writer.

## What Changes

Extend the existing PTY probe with a marked 512-turn replay, a 40 ms frame-writer delay, and measured key echo while replay is still in progress. Assert that the draft survives the session-loaded boundary and that measured key-echo p95 stays within 100 ms.

This is ignored PTY test coverage only. Product behavior and archived behavior contracts do not change, so this change uses `skip_specs: true`.

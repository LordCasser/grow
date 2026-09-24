## Why

The existing ignored resume latency probe measures a 512-turn history and a separate slow-writer replay, but its fixture has no dense historical tool-call/update traffic. Add a focused probe for key echo while resuming that heavier update stream.

## What Changes

- Add an ignored PTY probe that replays 512 turns with 512 synthetic tool calls and 512 corresponding tool updates, while retaining the 40 ms frame-writer delay and draft/marker assertions.
- Keep the existing resume latency probes unchanged.

## Scope

This is test-only instrumentation. It does not change application behavior or any archived contract, so this change uses `skip_specs: true` and contains no delta specs.

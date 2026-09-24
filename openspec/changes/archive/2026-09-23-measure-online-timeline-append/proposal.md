# Measure online Timeline append cost

## Why

The archived 66,069-event measurement covers replay. Live `prepare` and `accept` each validate transactionally and clone lifecycle state, so replay numbers do not establish online append cost.

## Scope

Measure successful online append latency and allocations across history and lifecycle sizes, and confirm rejected appends preserve state. Record whether evidence closes or narrows the corresponding backlog item. No behavior change is proposed.

## What Changes

Add a test-only ignored benchmark and record its evidence. Narrow the backlog item to the measured lifecycle-state scaling cost.

This audit changes no product behavior or accepted specification, so `skip_specs: true` is used and no delta is added.

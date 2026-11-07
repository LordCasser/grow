# Separate finalization readiness from retry attempts

## Why

Coordinator increments a pending child's three-attempt recovery budget before Shell can report that the original output I/O or actor/persistence owner remains alive. Three readiness observations can exhaust the period without dispatching a recovery operation. The existing thirty-second absolute period and explicit next-turn reopening are retained; this change improves convergence only inside that same period.

## What Changes

The Coordinator asks its existing ChildRunner for a synchronous, non-mutating readiness observation before admitting a reconciliation attempt. A false observation neither spends an attempt nor renews the deadline. Shell reports false while output I/O or its child writer incarnation remains alive, or no healthy effective parent writer is available. Reconcile retains all authoritative ownership, lease, identity and in-flight ACK checks.

No provider/Hook replay, new registry, persistent state, public tool schema, timeout extension or unlimited retry policy is introduced. Changes affect only finalization scheduling, its Shell adapter and related tests/docs.

## Context

The manager actor no longer awaits external classification or ACP prompts. Its control commands cancel scoped tokens, but the public synchronous methods only enqueue those commands. The public method return and the actual cancellation have no ordering relationship.

## Decision

Each `SetMode`, `ResetState`, and `ReleaseChild` command carries a one-shot acknowledgment. The handle awaits it. The actor sends the acknowledgment after updating local state and cancelling old tokens. `SetMode` publishes its atomic mode from the actor before acknowledgment, so the caller sees a committed mode. Requests stay independently spawned; no provider call runs in the mailbox. An unavailable actor yields a visible error log, while existing shutdown rejection remains authoritative.

This is a linearization barrier for callers, including UI mode events and child cleanup. No cross-thread token registry or duplicate permission state is introduced.

## Verification

Run the fixed-order control/classifier regression, existing cancellation and concurrent-request tests, Shell permission tests, strict OpenSpec validation, and check the resulting diff.

## Risks

Control APIs now await actor scheduling. The actor has no external wait in its command loop, so this is bounded by local event-loop progress. A control called from inside a request classifier can cancel that classifier before the acknowledgment reaches it; the outer request must still return `Cancelled`.

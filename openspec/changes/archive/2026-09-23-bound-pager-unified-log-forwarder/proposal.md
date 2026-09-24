## Why

Pager's `unified_log` drains each batch into a detached send task. `flush_blocking` waits only for entries still in its buffer, so an earlier batch can remain unacknowledged at exit. The pre-init buffer, individual entries and concurrent sends have no aggregate byte budget; a disconnected or slow ACP peer can accumulate work.

## What changes

- Give Pager log forwarding one FIFO sender with a bounded pending queue and bounded encoded entries/batches.
- Make `flush_blocking` wait for every accepted entry before its call to settle or for the existing two-second deadline, including entries previously dispatched by periodic or count-triggered flushes.
- Keep logging best-effort: oversized or over-budget entries are dropped with bounded diagnostics; send failure does not retry or block shutdown indefinitely.

## Impact

Only Pager's diagnostic forwarding behavior changes. Logging remains outside session/model state. The ACP transport and Shell log writer contract are unchanged.

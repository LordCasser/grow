## Why

The Linux v2.6.0 candidate freezes a restored child grant before the direct parent publishes its initial MCP connections. A later ready parent cannot repair the already-frozen empty binding. This loses a surviving birth member even though the durable membership fact is correct.

## What Changes

Cold child restoration obtains the direct parent pool only after its current initial MCP handshakes settle, then applies the existing birth-name/profile/parent-ceiling intersection and freezes current client identities. Normal child creation retains its immediate snapshot behavior. The wait is owned by the session and cancellation-aware, without a new timeout or retry policy.

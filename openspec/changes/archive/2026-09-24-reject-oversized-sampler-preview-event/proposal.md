## Why

The sampler handoff budget charges a single event at no more than its 4096-credit total, even if the event's payload itself is larger. Grow's normal HTTP attempt has a 64 MiB raw response-evidence stop, but that upstream fact should not be the only protection for a public event handoff.

## What Changes

Reject an oversized charged sampler event before enqueue instead of clamping its credit cost. The attempt fails with a local lifecycle error and ordinary failed-attempt handling; no oversized fragment reaches the Shell event queue.

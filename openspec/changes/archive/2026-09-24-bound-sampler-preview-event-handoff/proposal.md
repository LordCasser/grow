## Why

The session sampler forwards every text, reasoning, tool-argument, and signature fragment into an unbounded channel. Its single drainer awaits client and persistence delivery per event, so a slow consumer can accumulate arbitrarily many candidate fragments before canonical admission. The existing 64 MiB raw-response cap does not bound per-fragment queue overhead or repeated attempts.

## What Changes

Give the session sampler-to-shell handoff a 64 MiB / 4096-fragment credit budget. The L2 producer waits asynchronously for credits before forwarding a fragment; the session drainer returns them when it removes the fragment from the queue. Cancellation interrupts a producer waiting for credits. Attempt start/discard and terminal events retain their existing order and delivery semantics.

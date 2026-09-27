## Context

The manager's request channel and the root session's classifier channel are unbounded. The former counts in-flight calls but never caps them; the latter spawns one Sideband task per message. A single short actor loop does not block, yet unrestricted fanout can exhaust process/provider resources.

## Decision

Use the existing shared in-flight atomic as an admission gate: a child is admitted while the total is below 64, a primary request while the total is below 72. This reserves eight slots for primary work when child requests saturate and caps total fanout. Acquire before enqueue using a compare-and-update; move the guard into the actor command so cancellation releases it only after the actor discards the command or completes the decision. This bounds bursts of cancelled commands while the actor is busy. At the applicable threshold, fail closed immediately with a harness-owned denial. The root classifier worker independently holds at most 16 active permission Sideband permits, acquired without waiting; when full it returns a typed `Overloaded` classifier failure. Primary Auto follows its existing unavailable-to-human-prompt policy, while child Auto denies without a prompt. No FIFO is added for model waits. An admission rejection metric uses only two fixed stage labels and logs the cap, never request arguments.

The manager gate bounds all classifier-channel senders even though the channel type remains unbounded. Short control commands retain independent mailbox access, including at saturation. The thresholds are finite per-primary-session resource limits, not policy grants.

## Verification

Hold 64 child manager requests, verify the next child fails promptly and a primary local decision proceeds. Hold eight more primary prompts, verify the next primary fails promptly and cancellation frees capacity. Hold 16 Sideband tasks through a mock provider, verify the next gets typed overload and the first requests can complete independently. Re-run existing parallelism, control and permission tests.

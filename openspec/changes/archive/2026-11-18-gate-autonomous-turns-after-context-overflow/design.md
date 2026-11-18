## Evidence

The reported turn committed a replacement from roughly 665k to 461k tokens, then failed against a 272k model. A pending background completion created another Turn within 28 ms. That Turn started its own summary and held the control boundary for about 140 seconds. The larger model was applied after the second summary finished. There was no ordinary provider request to the small model between those summaries.

The existing idle arbiter already prioritizes controls and explicit foreground work. This did not protect against a new autonomous Turn starting before a recovery control arrived. Turn-local compaction suppression resets on each new Turn. Ctrl+C suppression has independent user-engagement semantics.

## Ownership and decision

The durable Turn terminal labels a typed context-window failure and records the effective window the failed execution used. Timeline derives a small recovery projection from that fact and subsequent history. Published working cuts preserve the same projection. A successfully completed provider Turn clears the failure. An admitted partial overflow response does not establish recovery. Committed compaction, rewind, and history rebuild supply evidence of changed history. Either changed history or a larger active window allows a fresh fit check, with the existing effective input estimate and configured output reservation. A failed or superseded control cannot change the inputs to this check. A larger window that still cannot fit the estimate leaves the work parked. The complete request still goes through ordinary provider and compaction admission.

The Session actor keeps its existing foreground and control ordering. Notification batching and Goal both check the shared autonomous model admission predicate at their claim boundaries. Notifications remain in the durable inbox during a hold. Goal waits for an external idle edge instead of waking itself while capacity is blocked. Explicit prompts, model controls, and manual compaction keep their normal admission path.

## Alternatives

A notification-only gate misses Goal and cold loading. A delay leaves the race between timeout and model selection. Clearing the hold when a picker accepts a model can use a superseded or uncommitted destination. Comparing only local pressure misses provider-reported failures below the configured window. The existing canonical terminal carries the evidence without an independent recovery journal.

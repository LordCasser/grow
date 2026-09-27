# Design

The subagent producer currently allocates an event ID before enqueueing the parent command. The parent actor later persists that projection, awaits the `SubagentStart` observe hook, emits a Goal refresh, and only then forwards the older spawn. Pager treats live Grow event IDs as an ordered highwater, so the Goal refresh can make the spawn appear stale.

The parent actor will finish the observe hook first, assign the lifecycle projection's event ID at its own publication boundary, persist it, and forward it before the optional Goal refresh. Moving the hook first prevents a long hook run from holding an already-stamped spawn while other updates advance the live stream. The canonical Timeline spawn is already durable before this command; a hook failure leaves that authority intact and withholds the derived Grow projection. The same stamped projection is still used for persistence and gateway delivery. Client-origin notifications remain persist-only, and transient progress remains unchanged.

If the parent command channel is unavailable, the producer retains its direct gateway fallback and stamps that fallback before sending.

An independent Goal or Workflow producer can still publish while the parent actor awaits a persistence ACK. Pager will therefore admit a late `SubagentSpawned` only when its child is absent and no terminal row for that child is already visible. It will admit a late `SubagentFinished` when the child is not yet marked finished. These lifecycle updates already carry stable child identity and use idempotent row logic. Applying a lower-ID lifecycle fact must not lower the Grow highwater or reconnect cursor. Other Grow kinds retain their existing highwater rule. This is a focused client reconciliation rule, not a new cache or protocol field.

Shell and Pager regression tests will cover the ordered common path, out-of-order live spawn and finish, duplicates, and cursor preservation.

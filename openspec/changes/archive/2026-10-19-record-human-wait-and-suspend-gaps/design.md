# Design

## Durable interaction identity

An interaction attempt is identified by `(tool_call_id, attempt_id)`. `attempt_id` is newly generated for each actual reverse-request attempt, including a rebind after suspension. The typed event records only the kind and terminal category. It does not retain user answers, questions, permission options, command input, or tool arguments.

Timeline validates an exact state machine: one Opened event per attempt, at most one matching Resolved event, matching tool-call identity and interaction kind, and no resolution without an open. An unclosed attempt remains open/unknown at the captured frontier. No cold-load repair invents a terminal timestamp.

The event is appended durably immediately before the ACP reverse request is issued. The corresponding resolution is durably appended after the request returns, including timeout, cancellation, malformed response and transport failure. Failure to persist Opened prevents sending the request. Failure to persist Resolved prevents the caller from treating the interaction as confirmed and remains visible as unresolved recovery responsibility.

Question and Plan approval dispatch already own their ACP boundary. Permission prompts are issued inside Workspace's `AcpPrompter`; Shell supplies a small request-local observer so the actual client prompt boundary is recorded there. Local policy decisions and automatic grants do not create human-wait intervals.

Question requests carry the existing tool wait budget as one absolute, in-process deadline. When the tool closes its reply receiver, the worker distinguishes expiry of that same deadline from an earlier cancellation; the deadline is not a second persisted timer. An unconfirmed resolution is returned as an infrastructure error, never as a confirmed answer.

## Time projection

Timeline `at_ms` is a persisted wall-clock timestamp, not a monotonic clock. Projection pairs facts by full attempt identity and event order. Negative or conflicting ranges remain unknown; no duration is inferred from `ToolStarted`/`ToolCompleted` alone. Historical Plan approval intervals continue to use their existing durable Control gate evidence.

For suspension, only a validated `TreeSuspended` and later `ExecutionReleased` with the same operation identity define the tree's offline interval. `Resumed` is not the end because admission remains closed until `ExecutionReleased`. Subtract this interval from otherwise protected work spans. If unrelated work overlaps the interval, preserve that work as protected. Recovery gaps remain a separate estimate and are never presented as confirmed execution time.

Apply the 30-second compression threshold after protected, recovery, and offline ranges split a candidate. Every final fragment must be strictly longer than 30 seconds; shorter pieces, including subsecond offline fragments, retain their source duration. This avoids stretching a tiny fragment to the fixed one-second playback window or making source-to-playback mapping non-monotone.

## Scope boundary

This change does not add periodic heartbeats, alter interaction UX, backfill old history, or create a separate wait store. It does not change tool outcomes or permit replay. Existing pending-interaction UI notifications remain transient projections of the live registry.

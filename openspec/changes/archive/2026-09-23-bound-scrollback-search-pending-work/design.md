# Design: Bound scrollback search pending work

The UI owns a single `DrainedUpdate` protected by a short mutex. Each submit merges its optional corpus, replaces query and request generation, then calls `try_send(())` on a capacity-one wake channel. A full wake channel means a worker wake is already pending; submission never waits for a scan. The worker takes the pending state before each scan, keeping its current corpus across requests that omit a corpus. An owner drop marks the pending state stopped and sends a nonblocking wake, so no queued query can overtake shutdown.

Results retain their existing request generation and query checks. The worker remains detached because joining an in-flight scan on the UI thread would stall input; it exits after the active scan observes the stop request or completes. The stop check is observational, not a hard scheduling deadline.

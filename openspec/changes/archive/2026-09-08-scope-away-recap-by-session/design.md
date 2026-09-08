# Design
FocusTracker already owns away timing and recap bookkeeping. Replace its single shown boolean with a set of session IDs and single attempt time with a map. This avoids a second tracker or session lifecycle framework. Clear both on focus loss as before; no persistence. The global away threshold remains terminal-scoped.

A shared active-session eligibility helper resolves the root session ID and feature/config gates before asking the tracker. Both polling and FocusGained use it, the latter before clearing the away timer. Dispatch records the actual Effect session; notification handling records the actual envelope session. Child automatic initiation remains unsupported, matching existing routing.

# Validation
Keep existing focus tests with an explicit session identity; add independent attempts, shown results and fresh-away reset across two IDs. Exercise a background live recap through actual ACP routing and verify current-session eligibility remains available. No real provider or focus event automation.

# Limits
Late results across away periods retain existing semantics; request/away generation correlation is separate. Per-away records clear on the next focus loss, not on tab close. Duplicate FocusLost behavior remains unchanged.

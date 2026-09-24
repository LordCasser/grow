# Design

## Decision

Use `AppView::reconnect_pending` as the admission boundary. `active_session_recap_due` returns false during reconnect, which covers both periodic pre-generation and the eligibility snapshot taken before `FocusTracker::on_focus_gained` clears the away period. `dispatch_send_recap` also rejects automatic requests while reconnect is pending, protecting the central request path from other or future automatic callers. Neither path records retry backoff.

Focus return during reconnect intentionally drops that away-period opportunity. This matches the existing best-effort contract: automatic request failures are silent and are not retried after focus returns. The next away period is eligible normally after reconnect if the refreshed capability remains enabled. Manual requests continue to use the existing prompt/reconnect guard and refreshed shell capability.

## Limits

Do not change when reconnect capability is applied, introduce pending intent, or add general capability invalidation. Session reload duration and automatic recap retry policy remain unchanged.

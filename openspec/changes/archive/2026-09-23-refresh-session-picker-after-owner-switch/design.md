# Design: Refresh a pending picker after switching owners

## Decision

The outer `dispatch` wrapper runs after every Action handler, including Agent and child focus changes. If the visible Welcome or Agent picker is still loading and its current view binding differs from the binding of the latest list fetch, dispatch one fresh list fetch. That operation updates the sequence and binding before the wrapper returns, so the next action cannot reissue the same request. A stale result may trigger the check, but it cannot update another view; a subsequent switch back to the original pending modal requests its list again.

This retains the single-current-fetch architecture. Already loaded modals need no refresh, and no background result is routed into an inactive view. The effect is returned through the ordinary dispatch result path, so no detached task or second state store is introduced.

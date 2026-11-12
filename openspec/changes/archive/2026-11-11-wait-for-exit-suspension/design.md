## Decisions

Pager keeps the ACP request in flight and shows the pending exit state until the owner responds. Waiting requires no further keypress. The pending notice exposes the existing explicit force-quit action with a recovery warning. Repeated Ctrl+D continues to coalesce.

The exit owner runs the existing tree suspension with no shared elapsed-time cutoff, including an already-draining writer. Other operations that deliberately supply a deadline keep it. The persistence writer's own failure policy and the terminal subsystem's bounded command retirement still apply. Startup rejection, verified tree membership, canonical operation identity, durable checkpoint order, and physical writer retirement still govern confirmation.

A failed suspension can be retried through the existing request path. A successful response must still come from the current request before Pager exits or restarts for an update.

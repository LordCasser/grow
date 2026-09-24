# Design

Use a process-wide atomic worker permit with a ceiling of two. Acquiring is nonblocking on the render path; when saturated, the request is left out of `pending` and can be retried on the next render. A per-view pending ceiling of two also bounds completion payloads retained by that view's mailbox, because paths remain pending until the UI applies their completion.

Workers open the file off-thread, reject metadata sizes above 16 MiB, and read through a `take(limit + 1)` guard to catch growth/replacement races. Prepared output above 16 MiB is discarded before mailbox admission; conversion may temporarily allocate up to the existing 100 MB preparation cap per worker. Existing 40-attempt/50 ms rename-race retry behavior remains unchanged. Each worker permit is RAII-released on completion, spawn failure, or panic. Reset still swaps the mailbox, so late results remain detached from the new session.

## Tradeoffs

Oversized or malformed media continues to use the existing failed-path behavior. Under worker saturation a visible image may wait until a subsequent render retry. The bounds apply per AgentView for mailbox payload count; a separate future audit may measure total cached bytes across simultaneous views.

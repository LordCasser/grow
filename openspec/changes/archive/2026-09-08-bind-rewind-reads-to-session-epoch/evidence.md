# Evidence

- AgentView::bind_session_id increments session_binding_epoch only when the ID changes; unbind_session_id increments on an existing binding. Repeating bind for the same ID is intentionally a no-op for the epoch.
- Current rewind_read stores UUID plus SessionId; accept_rewind_read compares the current session ID. This does not distinguish unbind/rebind of the same ID.
- Production direct assignment to session.session_id is inside bind_session_id; load failure/stale owner paths call unbind_session_id. Fork/worktree completion call bind_session_id.
- Existing task-result ownership code already consumes session_binding_epoch for other asynchronous interactions.

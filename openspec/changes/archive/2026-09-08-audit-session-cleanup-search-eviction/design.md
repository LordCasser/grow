# Evidence
- Manual delete_session_history obtains removed Info and calls notify_session_updated. The search worker reloads the missing summary, then delete_session removes its index document.
- Automatic cleanup_stale_sessions_sync currently returns only deleted/error counts; neither it nor the cleanup wrapper publishes deleted session identities to search.
- execute_search queries the SQLite index and returns its results without checking every result against current disk existence.
- bootstrap_once rechecks the completion marker on subsequent calls, and a present marker clears the flag without rebuilding. An initial bootstrap can also snapshot IDs before cleanup deletes them. Thus bootstrap is not a reliable substitute for deletion notification.
- enqueue remains available while indexing is disabled; the missing-summary deletion branch bypasses the indexing gate and delete_session is designed not to create a missing index. Existing test_evict_removes_row_and_never_creates_index covers that path, inspected but not rerun in this audit.
- Deleted automatic-cleanup sessions can leave stale search rows until another eviction/rebuild occurs. This is source evidence; no live stale search result reproduced this turn.

# Repair boundary
Carry successful deletion identities out of the cleanup operation (streamed callback or small result as appropriate) and invoke existing root-scoped search eviction scheduling. Do not bind a low-level explicit-root adapter to global GROW_HOME, invent a second index store, or emit deletion for protected/error candidates. Preserve asynchronous indexing semantics rather than promising instant consistency.

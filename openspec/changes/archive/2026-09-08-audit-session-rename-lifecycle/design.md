# Evidence
- load_session calls begin_session_load then acquires lock_session_lifecycle through restore/replay/spawn/publication. Delete holds teardown_live_session_before_delete's returned guard through disk removal.
- handle_session_rename validates title, lists summaries, checks session_handle_waiting_for_load, then sends SetSessionTitle to a live actor or calls append_session_title_durable for a dormant session. It never takes the lifecycle guard.
- The dormant append reads/folds Timeline, prepares the next title event and asynchronously appends it. A load can begin after the resident-handle check; delete can run after summary lookup. Existing serialization does not encompass rename's lookup/write decision.
- Possible outcomes include stale lookup, failed write or competing writer state. No actual duplicate sequence, resurrection or production data corruption was reproduced in this audit.
- A naive lock addition followed by session_handle_waiting_for_load is unsafe: begin_session_load precedes lock acquisition, so a queued loader may be announced while waiting for the guard held by rename. Waiting for that loader under the same guard would stall.

# Repair boundary
Take the existing per-session lifecycle guard before authoritative summary/actor resolution and hold through title commit. Resolve the resident actor directly while guarded; do not wait for an announced loader that is blocked by this guard. Preserve live-actor canonical title handling, dormant append semantics and search notification. Scope is one MvpAgent lifecycle gate, not a new cross-process lock guarantee.

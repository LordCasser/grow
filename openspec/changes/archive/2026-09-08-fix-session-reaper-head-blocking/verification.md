# Verification
Initial full Shell suite: 3765 passed, 1 failed, 3 ignored; reaper_stops_persistence_after_last_owner_thread_exit timed out. That test passes in isolation. Production reaper source shows FIFO blocking join.

Deterministic pre-fix regression reaper_completed_session_is_not_blocked_by_live_predecessor fails after 2 seconds while the predecessor remains blocked; the test releases it before asserting.

After the fix, the full Shell library suite passed: 3767 passed, 0 failed, 3 ignored (85.91 seconds). This includes the deterministic blocked-predecessor regression and existing final-owner, join and persistence-stop lifecycle coverage. Log: /tmp/grow-shell-core-after-reaper-fix.log.

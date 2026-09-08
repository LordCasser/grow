## Why
Snapshot consumer audit found test_leader_stdio_integration explicitly permits real-home unified-log writes. Shell unit-test ctor is cfg(test) and does not protect this integration binary.

## What Changes
Add pre-main unified-log redirection to this test binary and verify in an exact isolated child process. Correct stale real-home comments. Test-only infrastructure; production contract unchanged, no delta. Record unused snapshot_session_log as R26 pending confirmation.

# Evidence

Source inspection found cleanup collecting every OpenedSession before deletion, while the scanner skips directory-open and summary-read errors. The isolated production regression (96 actual expired sessions, child-only soft RLIMIT_NOFILE=64) failed: cleanup returned (49, 0) instead of (96, 0). Parent red result 0 passed / 1 failed in 4.21s; child 4.17s. This demonstrates incomplete cleanup reported without candidate errors, not an explicit top-level EMFILE.

The fix keeps discovery before deletion but retains summaries only. Each admitted candidate gets an adapter sharing the pinned root and directory mode with fresh operation caches, so maintenance resources are dropped for both successful and preserved/rejected candidates. Current identity, format, hidden state and activity are checked under writer ownership before deletion of the pinned entity.

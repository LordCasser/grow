# Evidence

scan_opened_sessions swallowed all cwd/session directory-open and initial summary errors, and used is_err on physical identity validation (including long-CWD marker IO). Search reindex_all obtains list_sessions before its indexing/pruning/completion logic. Cleanup consumes the complete scan result before entering its deletion loop.

Red test operational_scan_errors_preserve_cleanup_candidates: 0 passed / 1 failed (0.17s) on the first case, unreadable CWD directory. The permission probe confirmed PermissionDenied, while list_sessions_sync returned Ok containing only the readable session. Permissions were restored before assertion. The red run stopped at that assertion; no claim is made that all four cases individually failed on old code.

Operational errors now propagate unchanged; NotFound and InvalidData exclusions remain. Existing summary-format/hidden/cwd filters are unchanged.

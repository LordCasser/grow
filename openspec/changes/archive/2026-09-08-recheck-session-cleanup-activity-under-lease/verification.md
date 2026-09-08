# Verification
- main, locked/offline shell --lib, incremental/dev-debug/test-debug off, jobs 2, RUST_MIN_STACK=16777216.
- cleanup_rechecks_activity_after_scan_under_writer_lease: 1 passed, 0 failed, 0 ignored, 0.32 s; seven isolated temporary session fixtures.
- For refreshed/future/corrupt/identity cases, capture the actual scan result, then alter that entity's summary before invoking the production lease/recheck/delete helper. Fresh/future activity is retained, corrupt and changed-identity summaries return errors with unchanged bytes. This is deterministic ordering, not a probabilistic cross-process race reproduction.
- Full cleanup loop deletes a truly expired fixture, preserves an independently held writer lease, and honors explicit skip. Initializer leases are released except in the held-writer case. No real user history cleanup or access.
- cleanup_ttl: 3 passed, 0 failed, 0 ignored, 0.00 s, retaining checked configuration/date boundaries.
- Existing pinned-directory, bounded summary read, identity/quarantine checks remain. Noncooperating writers and Windows not tested. Lease retention follows existing adapter-cache lifetime, including preserved candidates.
- Existing large __eh_frame compact-unwind linker warning; all selected tests passed.
- Strict all 16 / archive 241 passed. cargo clean removed 7,372 files / 2.7 GiB; final available disk 64 GiB.

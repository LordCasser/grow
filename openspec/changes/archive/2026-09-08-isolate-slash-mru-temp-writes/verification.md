## Verification

`cargo test --locked --offline -p pager --lib slash::mru --quiet`:10passed,0failed,0ignored (0.24s). Build used disabled incremental/debug information, two jobs and RUST_MIN_STACK=16777216. Existing macOS compact-unwind warning appeared; command exited0.

New tests verify an existing fixed .json.tmp file remains byte-for-byte unchanged after successful publication. A nonempty destination directory forces publication failure; its contents and the unrelated fixed temporary file remain, and no owned temporary file leaks. Four direct writer threads publish12 snapshots each through independent handles; every observed destination matches one complete expected JSON snapshot and only the destination remains afterward.

The direct-writer test bypasses the process-local queue to exercise concurrent filesystem publication; it is not a launched multi-process test. Existing recency, canonical name, dirty snapshot and retry tests pass. All writes use temporary test directories; no real slash-mru.json or environment override was used. No Windows execution or crash/directory-fsync test was performed. Cross-process snapshot merging is unchanged and not claimed.

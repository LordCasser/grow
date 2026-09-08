# Verification

## Implementation
The startup wrapper runs a same-handle ordinary-file reader in spawn_blocking and maps read/join failure to the existing empty set. Metadata is checked before a take(limit+1) reader; byte count is checked before UTF8/JSON parse. Unix O_NONBLOCK allows special-file admission without waiting for a FIFO peer. The normal writer checks encoded length before preparing its atomic replacement.

## Coverage
Explicit-path tests use a canonical multibyte-ID JSON document padded exactly to1MiB, roundtrip it, reject an oversized replacement while preserving prior bytes, reject an oversized on-disk source without modifying it, and prove a larger Cursor consumes onlylimit+1. Malformed JSON stays fail-open; a directory is rejected. Unix tests create a FIFO without a writer, require prompt rejection under a watchdog and preserve the FIFO, then verify a symlink to a regular file loads normally. Existing atomic-write tests remain in the same library run.

## Limits
No GROW_HOME/env changes or real user data. No live startup or Windows run. The reader-consumption test directly exercises the helper used after metadata inspection, not a scheduler-controlled concurrent file-growth test. No ordinary-filesystem IO deadline or upstream configuration/serialization memory cap. Async reader rejection maps to empty via inspected production wrapper.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p announcements --lib --quiet: 13 passed, 0 failed, 0 ignored; 0.06s; process exited0. A post-test edit only moved the read documentation onto its function and clarified rejected-input wording. git diff --check passed.

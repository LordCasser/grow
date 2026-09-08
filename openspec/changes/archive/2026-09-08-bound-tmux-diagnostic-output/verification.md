# Verification

## Before / after
The old drain accepted a 65,537-byte stdout payload; the new regression failed before implementation. Production now caps each stdout/stderr drain using Read::take(64 KiB + 1), treating the extra byte as overflow rather than valid truncated data. An AtomicBool wakes the existing polling decision on its next iteration; overflow terminates/reaps via the existing process-group path. If the leader exits first, drain result collection still reports overflow.

The private command execution boundary is shared by the production tmux builder and the real-process tests; new tests do not modify PATH or invoke the user's tmux server.

## Tests
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager-render --lib tmux_probe --quiet`: 7 passed, 1035 filtered out, 1.33 seconds.

Coverage: empty, exact 64 KiB and one-byte excess for each stream; overflow flag; actual /bin/sh fixtures producing excess stdout or stderr then waiting; errors arrive before the 10-second process deadline (test bound 5 seconds), and signal-0 checks confirm the owned child no longer exists. Existing near-deadline success with inherited pipes and result parsing remain covered.

No real tmux configuration was queried or modified by the new tests. The pre-existing near-deadline test uses its isolated fake tmux and restores PATH. No installed CLI update. This adds per-stream byte bounds, not an end-to-end deadline for all doctor probes or a new generic process manager.

# Verification

## Implementation
Daemon now owns one pending Msg behind a short mutex and a capacity1 wake channel. submit merges item refresh/query before try_send; full notification capacity already means a wake is pending. Worker takes the message before building items, matching, sorting and publishing. Drop submits Stop without joining matching. Stop dominates pending data. Existing selected() deletion candidate is untouched.

## Coverage
New deterministic test creates a Daemon with a live but unconsumed notification receiver, sends an item refresh and1,000 queries, checks only latest combined items/query remains, then drops the daemon while the wake channel is full. Completion must arrive within2seconds; pending is Stop and only one wake is queued.
Second test checks fresh items reset an older query, following query retains new items, and Stop dominates either merge position. Existing history tests exercise the actual spawned matcher, item/query combination, navigation, query filtering and input acceptance.

## Limits
No OS-level slow matching injection or installedCLI exercise. Mutex acquisition is not lock-free; no expensive matching/fileIO occurs under pending lock. Running match is not cancelled, no join/flush is promised. H2 stale snapshot/request identity and D1 draft reads remain separate unresolved work.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib history --quiet:48 passed,0 failed,1 ignored;0.08s; process exited0. The ignored existing leader_kill_reconnect_reloads_without_duplicating_history requires isolated process-global environment and is outside this search-daemon change. Existing macOS compact-unwind warning only. git diff --check passed.

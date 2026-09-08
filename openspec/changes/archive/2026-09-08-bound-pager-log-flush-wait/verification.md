# Verification

## Implementation
Only the existing current-batch acp_send wait in Forwarder::flush_blocking is wrapped with a two-second Tokio timeout. No retry or extra task is created. Existing pre-init retention and threshold dispatch remain unchanged. Callers still perform terminal restoration and agent cleanup after returning from this wait.

## Test design
The retained-ack test runs the production flush, receives the actual ExtNotification, and keeps response_tx alive while waiting beyond its deadline. It checks elapsed time, receiver closure, failed late acknowledgement, empty buffer and absence of a retry. An independent outer watchdog prevents indefinite regression hangs. A disconnected-channel test requires completion inside one second. Existing tests cover ordinary-thread dispatch and a real acknowledged startup batch through the ACP oneshot.

## Limits
This is a two-second asynchronous delivery-wait budget, not a hard whole-process wall-clock guarantee. Synchronous serialization, poisoned/contended mutexes, stalled runtime scheduling, terminal restoration and agent cleanup have their own behavior. Already enqueued notifications may be processed after timeout. Earlier detached batches are not joined; their queue/task budget remains separate. No real logs, installed CLI, Windows or live terminal validation.

## Results
CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2 RUST_MIN_STACK=16777216 cargo test --locked --offline -p pager --lib unified_log --quiet: 4 passed, 0 failed, 0 ignored; 2.01s; process exited 0. Existing macOS compact-unwind linker warning only. git diff --check passed.

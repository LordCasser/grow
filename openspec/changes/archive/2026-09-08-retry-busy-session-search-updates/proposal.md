## Why
flush_ready removes a pending session key before upsert_by_key runs. A SQLite lock-contention failure after the driver's busy timeout is only logged, losing that update/eviction until an independent notification or full rebuild. Completed-bootstrap markers do not themselves retry the lost key. sqlite_to_io_error currently erases the SQLite error classification.

## What Changes
Preserve SQLite Busy/Locked as an explicit retryable classification and reschedule those failed keys through the existing per-root pending queue. Keep permanent errors distinct; do not blanket-retry invalid history or permission failures. Retain asynchronous projection semantics without introducing a durable outbox.

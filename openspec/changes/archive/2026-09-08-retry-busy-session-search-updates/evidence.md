# Source evidence (main, 2026-09-08)

- session/storage/search.rs::flush_ready removes each ready key before awaiting upsert_by_key; Err only calls log_session_index_failure.
- sqlite_to_io_error converts every rusqlite error into io::Error::other with formatted text, erasing code-based classification.
- search_fts::with_index retries only errors classified as an unusable/corrupt DB; Busy/Locked is returned. JournalMode setup includes a busy timeout, so this concerns contention outlasting the driver's own wait, not every ordinary overlap.
- Missing-summary eviction and regular session upsert converge on the same worker path. After a completed bootstrap marker exists, repeated search rechecks can adopt it instead of rebuilding, so they do not guarantee repair of this dropped key. A later independent update or future launch rebuild can still repair it; no permanent data-loss claim is made for canonical session history.

No runtime lock reproduction, build or user-history mutation has yet occurred for this change. Actual contention behavior and error propagation remain to be verified with a temporary index before implementation.

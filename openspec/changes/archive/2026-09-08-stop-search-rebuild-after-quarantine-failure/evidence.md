# Source audit (main, 2026-09-08)

search_recovery.rs::quarantine_db_files moves -wal/-shm/-journal first, logging rename errors and continuing, then handles the main rename as a bool. It returns None both for absent main and failed main rename. heal_unusable calls recreate unconditionally after this helper. If quarantine succeeded but recreate failed, it increments CACHE_EPOCH and emits a warning saying the cache was recreated empty.

CACHE_EPOCH also guards callers against cache replacement, so simply withholding its increment on a failed recreation would need careful review: isolation may already have changed the live namespace. Do not conflate invalidation with successful healing. Existing tests cover successful moves and reprobe rejection, not failed isolation.

No failing runtime fixture, build or real cache mutation has been performed in this audit. No claim is made that old sidecars have actually contaminated a new database; the established finding is that the recovery control flow does not stop on isolation failure.

## Implementation and regression evidence

The deterministic macOS fixture uses a legal 240-byte basename whose quarantine destination exceeds the component limit. Before the fix, `quarantine_failure_does_not_recreate` failed (0 passed, 1 failed): recreation ran after rename failed. All files were under TempDir.

The helper now propagates rename errors, treating only NotFound as absence. Any successful rename invalidates CACHE_EPOCH even if a later rename fails. Inspection of CacheEpoch/current_epoch consumers confirmed this counter tracks namespace changes rather than successful heals. No partial rename rollback is attempted.

heal_unusable returns retry permission: a healthy reprobe or successful recreation permits retry; lock, transient probe, isolation and recreation failures deny it. Both search_fts callers return the original triggering SQLite error when healing denies retry. Diagnostics record the isolation/recreation error separately.

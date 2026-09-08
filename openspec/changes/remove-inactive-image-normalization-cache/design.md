# Reviewed boundary
Repository references show apply_remote_settings_side_effects is only defined, never called. NormalizeCache::global therefore always bypasses its moka cache. Only tests enable per-case caches. Actual production path still runs compute_normalized and run_blocking.

Move the existing worker semaphore/error and raw normalization outcome into image_normalize, remove cache indirection and the raw fingerprint helper used exclusively by this cache. Preserve the cancellation-retains-permit test. Remove cache-hit/TTL/weight-only tests. Keep actual format, metadata, bounds and oversized-routing tests; exercise fallback-note collection through the same outcome collection method used by normalize_images rather than seeding an inactive cache. No cache dependency removal unless independently unused; image-description caching remains active.

This cleanup is one R27 deletion commit with its own regression. It does not claim the requested per-model image/description and OCR feature is complete.

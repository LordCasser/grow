# Evidence
- image_normalize::normalize_images uses NormalizeCache::global; constructor sets enabled=false, disabled get_or_try_insert_with directly computes.
- Config-types exposes image_normalize_cache_enabled. The only production reference is agent::config::apply_remote_settings_side_effects, which calls global().set_enabled(...). Full repository Rust symbol search finds no caller of this function. Other set_enabled calls are test-only. Thus current repository runtime has no identified activation path; presence of a configuration field is insufficient.
- Enabled branch uses content-keyed moka cache, configured64MiB weight,1hTTL/15minTTI. It re-stamps per-call compression index and preserves current input metadata in entry_to_outcome. Disabling is a bypass, not invalidation; an already admitted computation can finish in cache. That alone is not a demonstrated correctness bug because compute is content-derived with fixed parameters.
- Non-native format transcode occurs before cache admission, so even a future enabled cache does not deduplicate that conversion phase. Byte capacity is an eviction policy, not total allocator/queued-image memory guarantee.
- Prior serialize-image-normalization-workers remains active for both uncached and enabled paths; do not delete its admission control or NormalizedEntry/NormalizeError used by actual normalization.

# Disposition
R27 proposes optional cache mechanism and disconnected activation hook/field for user review. No silent enabling or deletion. If retained, a separate implementation should explicitly choose configuration authority and wire/test startup/update application; this audit does not fabricate a setting contract. External users invoking the public hook are unknown.

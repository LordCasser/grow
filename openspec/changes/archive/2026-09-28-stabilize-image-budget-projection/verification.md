# Verification

## Reproduction and regression

Before the actor change, `CARGO_BUILD_JOBS=2 cargo test --locked -p chat-state --lib image_budget_selection_survives_append_until_next_high_water_mark` failed at “old selection must retain the third image”: after the first reclamation, append rebuilt from original images and advanced the eviction boundary early. After the fix, the same test passed. Its fixture uses two image parts per User item, checks the effective and newly evicted source coordinates, repeated build, two appends below the high-water mark, the next high-water mark, unchanged cache key/epoch on the stable append, native reset, immutable source images, and a cold actor rebuilding its own selection.

`CARGO_BUILD_JOBS=2 cargo test --locked -p chat-state --lib image`: 35 passed. This includes the three-backend ToolResult path with two image parts, continued append after reclamation, image description/removal projection, Timeline identity and rewind behavior, mixed carriers, size estimates and prefix behavior. `CARGO_BUILD_JOBS=2 cargo test --locked -p sampling-types --lib image`: 21 passed, including attachment lowering for all three backends.

The selection is actor-local and validated by `SurfaceId`, exact source Arc identity, model image-input key, Goal tag and description presentation. `ReplaceSamplingRoute` and a route-key-changing `UpdateSamplingConfig` clear it; ordinary config changes preserve it. New actors start with no selection, while a resident actor retains it. The sampler's existing `encoded_request_size_is_checked_before_dispatch` test covers the final wire limit, including the case where image reclamation cannot reduce non-image bytes.

`CARGO_BUILD_JOBS=2 cargo test --locked -p sampler --lib encoded_request_size_is_checked_before_dispatch`: 1 passed. `openspec validate --all --strict --no-interactive`: 18 items passed before archival.

## Scope and limits

These tests establish local request projection and encoded-size behavior. They do not establish remote cache hits, TTL, or provider routing affinity; those require `evaluate-provider-cache-affinity`. This change adds no Timeline mutation for budget choices. The request evidence exposes the full effective `evicted_parts` plus this build's `newly_evicted_parts`.

Cargo test artifacts were created under `target/`; this task will clean them after all verification is complete.

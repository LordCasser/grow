## Why
R27's NormalizeCache is disabled by default and its only production setter belongs to an uncalled remote-settings hook. The user authorized reviewing this deletion while requesting an explicit image/description fallback path.

## What Changes
Remove only the inactive process-wide normalization cache, its uncalled setter hook/flag and cache-only fixtures. Keep actual normalization, byte/dimension limits, oversized fallback feedback, metadata preservation and cancellation-safe single-worker admission. The session description cache and the new image fallback feature remain separate.

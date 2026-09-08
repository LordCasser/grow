## Why
FileStateTracker keeps lazy_source after a historical read failure but ensure_historical_loaded returns unit. Full-history getters and mutation helpers therefore continue with an incomplete in-memory set. Shell rewind and pending transaction recovery consume that set for file/projection/Timeline changes. A retained retry source alone does not stop the current unsafe operation.

## What Changes
Return historical-load errors through full-history queries and mutations and through shell rewind/recovery before side effects. Preserve the source and current in-memory points for retry. Keep lightweight metadata fallback and live point capture separate.

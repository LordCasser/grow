## Why
try_read_dropped_paths collects all decoded images before caller IMAGE_CAP checks. The per-file read limit leaves retained memory proportional to file count.
## What Changes
Bound successfully classified image bytes retained per paste to 50,000,000. On aggregate overflow return an empty classification for the entire paste, preserving caller text fallback instead of partial image delivery.
## Impact
Shared drop classifier and its image-only wrapper. No changes to attachment lifecycle or per-file limits.

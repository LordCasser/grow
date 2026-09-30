## Why

The normal session status and `/usage` show a measured cache-hit percentage, but an active Goal replaces that status with a Goal token summary. Goal details and the read-only transcript projection show cache-hit and cache-miss counts without a percentage. This makes the same usage harder to read when Goal is active or replayed.

## What Changes

- Show the Goal-specific cache-hit rate in the Goal status chip, Goal detail overlay, and read-only Goal transcript details.
- Calculate only from classified input, keep unclassified and historical input out of the denominator, and label partial measurements.
- Reuse the existing percentage formatter and add focused cases for exact, partial, and unavailable rates.

Plan and Workflow without a Goal already use the ordinary session status, which displays the cache rate. Goal accounting and persistence remain unchanged.

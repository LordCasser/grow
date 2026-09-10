## Why
Long-session resume repeatedly rebuilds Timeline. Each accepted event clones the cumulative lifecycle maps and sets before validation, making replay scale poorly as the history grows. The reported session has approximately 194 MiB of Timeline and 98 MiB of display updates.

## What Changes
Measure replay on a read-only frozen session prefix and optimize the confirmed lifecycle replay overhead. Preserve strict validation and atomic public live writes. Reuse shared validation/application code; no persisted caches, schema changes, log truncation or skipped history. Record additional bottlenecks separately if outside this bounded change.

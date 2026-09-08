## Why
Dashboard dispatch and peek use the image-only drop wrapper. A paste containing a PNG and ordinary file recognizes both, then filters out the ordinary path and returns early after attaching the PNG.
## What Changes
Use the shared mixed classifier in synchronous bracketed/key paste and deferred file URL completion. Insert NonImage paths as text in source order, using the existing target text insertion routine; retain image insertion and error handling.
## Scope
Dashboard mixed drops only. Question-mode guards and stale target protection remain. Deferred unclassified URLs with no original text are a separate backlog item.

## Why
Deferred file URLs are separate from original clipboard text. When classification returns no entries (including batch-budget refusal) and original text is absent, Agent and Dashboard currently discard those URLs and report FullMiss.
## What Changes
On a successful no-raster probe with unclassified file URLs, insert the raw URLs as text if the source has no non-whitespace original text. Reuse existing target insertion routines and completion variants. Existing source text keeps precedence and is not duplicated.

# Enforce the unified log file limit on each append

## Why

The existing 5 MiB check runs on a two-second maintenance cadence. Several Shell processes or a burst of records can therefore grow the shared `unified.jsonl` well beyond that threshold before any writer trims it. The new bounded queue isolates caller latency but does not constrain disk use.

## What Changes

- While holding the inode lock already shared with trimming, each writer checks the current file length plus its complete queued record(s), trims old complete lines if necessary, and appends only when the resulting file fits within 5 MiB.
- If the tail cannot be trimmed at a complete line boundary or the capacity check fails, the writer drops the record through its existing diagnostic failure path rather than exceeding the limit.
- Keep the existing bounded tail retention policy and inode-preserving trim. External writers that do not use Grow's inode lock are outside this guarantee.

## Why

Unified log appends now coordinate with in-place trimming, but a single message or context value can still serialize into an arbitrarily large line. The bounded trimming window intentionally leaves a file unchanged when it contains no newline, so one oversized record can defeat retention and allocate an unbounded serialization buffer before it is written.

## What Changes

Bound each serialized unified-log JSONL record, including its terminating newline, to 64 KiB. Serialize through a writer that rejects bytes beyond the budget before extending its buffer. Replace an oversized record with one complete, bounded diagnostic record that identifies the omission; never append a partial serialization.

## Capabilities

### Modified Capabilities

- client-surfaces: bound unified-log record serialization and define overflow diagnostics.

## Impact

Only the shared diagnostics unified-log serialization boundary and focused regressions change. The synchronous file-lock and write deadline remain a separate backlog item.

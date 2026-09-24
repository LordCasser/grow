## Why

Unified log trimming rewrites and truncates the current inode under an exclusive advisory lock, but `write_lines` appends without taking that lock. An append between rewrite and truncate can be removed by the truncation, losing a complete diagnostic line even though the writer reported success.

## What Changes

Require unified log append writes to use the same opened-inode advisory lock as trimming. Keep the current inode-preserving trim and best-effort diagnostics behavior.

## Capabilities

### Modified Capabilities

- client-surfaces: coordination of unified log append and trim operations.

## Impact

Only the unified diagnostic writer and a focused race regression. Long single-record size, cross-process log retention and synchronous I/O latency are outside this change.

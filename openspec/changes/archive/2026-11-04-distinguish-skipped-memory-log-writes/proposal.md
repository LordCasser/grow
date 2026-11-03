# Distinguish skipped memory log writes

## Why

`MemoryStorage::write_daily_log` intentionally skips workspace writes for ephemeral working directories, but currently returns the would-be path as though the write succeeded. Memory flush then reports `written`, reindexes that absent path, and advances its success checkpoint. Session-end summaries have the same false receipt.

## What Changes

Return an explicit written or skipped result from daily-log storage. Preserve ephemeral write suppression and propagate a skipped result through flush and session-end handling without claiming a file was written. A skipped flush remains eligible for a later retry.

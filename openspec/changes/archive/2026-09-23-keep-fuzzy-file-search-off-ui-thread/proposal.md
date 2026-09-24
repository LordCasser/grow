# Change: Keep fuzzy file-search worker teardown off the UI thread

## Why

The fuzzy matcher daemon uses a capacity-1024 synchronous channel. `set_query` and `restart_walk` wait when it fills, even though the worker may be joining a slow filesystem walk. Dropping the daemon also sends into that channel and joins the worker, so dismissing or retargeting file search can block the Pager input thread on filesystem activity.

## What Changes

- Coalesce file-search restart and latest query in one pending slot and use a nonblocking one-slot notification.
- Make daemon Drop stop pending work and cancel the current walk without waiting for the worker thread; the worker owns and retires its matcher.
- Preserve request identity and the existing restart-before-query behavior while replacing obsolete queued work. Workspace status polling rejects an older query snapshot and does not stall on generations skipped by coalescing.

## Impact

Workspace fuzzy file-search daemon, its status polling, and Pager callers; no durable or external protocol change.

# Change: Bound scrollback search pending work

## Why

The scrollback search worker scans off the UI thread, but its unbounded channel retains every query submitted while a long scan runs. Coalescing only happens after the worker finishes; a fast input burst can therefore retain arbitrary query strings and corpus references. Closing search enqueues `Stop` behind that backlog.

## What Changes

- Retain one merged pending search request and a one-slot nonblocking wake notification.
- Preserve the newest corpus update with the latest query; closing search takes precedence over pending work.
- Verify worker completion, burst coalescing and stale-result rejection through the real search state.

## Impact

Pager scrollback search worker only; no persistent or external protocol change.

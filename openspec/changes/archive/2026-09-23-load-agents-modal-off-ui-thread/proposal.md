## Why

Opening `/config-agents` currently reads effective config, resolves the default agent, and recursively discovers agent definitions on the UI event path. A slow project or user filesystem therefore delays the modal and all key handling. Toggling a row needlessly repeats the discovery scan, and returning from the external editor scans synchronously again.

## What changes

- Open the modal immediately in a loading state and discover its catalog in one bounded blocking worker.
- Correlate the result with the current modal instance so a close/reopen or late worker cannot replace newer state.
- Refresh after external editing through the same worker. Toggling only updates the row just written instead of rescanning the filesystem.

## Impact

Pager agent configuration modal only. The `/agent` picker and prompt construction have a separate discovery boundary and remain in the backlog.

## Why
Linux core CI fails to durably record unsupported-image capability because the local Resources store fsyncs an O_PATH directory handle.

## What Changes
Open a readable directory descriptor relative to the pinned capability on Unix before syncing. Preserve atomic publication and published-error classification.

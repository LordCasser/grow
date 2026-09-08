## Why
Linux milestone CI fails builtin extraction because cap-std directory capabilities use O_PATH and cannot be synced. A single extraction transaction fails before publishing its marker.

## What Changes
Open a readable directory descriptor relative to the pinned directory capability before syncing. Preserve atomic file rename, locking, symlink checks and marker-last publication. Add a first-call transaction regression.

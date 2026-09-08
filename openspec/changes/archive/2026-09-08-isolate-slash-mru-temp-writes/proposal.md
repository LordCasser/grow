## Why
Slash MRU snapshots use a shared slash-mru.json.tmp path. The background writer serializes within one process only. Concurrent Grow processes can truncate or rename the same temporary file; a renamed file can still be modified through another writer's open handle.
## What Changes
Give each snapshot write a unique owned temporary file in the destination directory, sync it and atomically persist it to the MRU path. Failure cleanup only removes that writer's temporary file. Preserve complete-snapshot last-writer-wins behavior; do not add cross-process history merging.

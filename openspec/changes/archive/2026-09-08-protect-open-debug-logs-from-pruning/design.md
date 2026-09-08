# Design
Use standard File locks already used by unified_log, no new dependency. Appender opens read+append to support shared lock on Windows, acquires shared lock before starting worker, fails through existing logger-open error behavior if unavailable. File remains owned by worker until guard shutdown. Concurrent writers can share locks. Cleaner only admits ordinary log files and skips lock/open failures; after exclusive lock, read handle mtime again before unlink. Existing latest name exclusion and age-based orphan temporary handling remain.

No protection against noncooperating old binaries or hostile path replacement, no byte quota or periodic sweep. Some filesystems may not support locks; opening debug output there can fail best-effort instead of silently losing protection. Windows behavior requires separate platform testing.

Use exact child process to invoke actual prune on a parent-owned temporary directory while parent retains writer. Verify old idle log remains, then guard release permits pruning. Do not touch GROW_HOME.

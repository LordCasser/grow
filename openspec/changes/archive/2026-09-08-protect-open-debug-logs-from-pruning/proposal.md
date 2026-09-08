## Why
Pruning uses mtime alone; an idle writer can remain open longer than seven days and lose its pathname when another process starts logging. Existing recent-file test does not cover idle open files.

## What Changes
Debug appender retains shared advisory file locks for writer lifetime. Pruning obtains a nonblocking exclusive lock on regular log candidates and rechecks handle age before unlink. Keep orphan symlink cleanup separate.

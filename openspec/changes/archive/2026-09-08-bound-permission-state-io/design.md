# Design
Use private synchronous reader behind spawn_blocking, keeping permission actor IO asynchronous. Check opened handle type and metadata length, then cap actual bytes with Read::take. UTF-8 decoding remains before TOML parsing. Join failure maps to IO Other, thus default remembered state. Unix O_NONBLOCK avoids FIFO open wait; regular symlinks remain supported.

1 MiB is a serialized permission-cache budget, independent of in-memory grants. Apply the same budget after serialization and before invoking atomic writer to avoid producing unreadable state. No overall memory/serialization, disk directory quota, ordinary filesystem deadline or FIFO guarantee on Windows. No common reader framework.

Verify exact boundary, oversized source preservation and no shared fallback, actual stream limit+1, oversized write preservation, and Unix FIFO with watchdog plus regular symlink. Tests use explicit temp paths.

## Decision

Use `tempfile::SpooledTempFile`, already in the workspace dependency set, for one leader candidate per session. The spool retains complete serialized ACP payloads in arrival order. It holds at most 8 MiB in memory before the tempfile implementation spills to an unlinked disk file. Count each record's length prefix and bytes against a 512 MiB per-candidate limit and cap record count separately. These caps cover serialization amplification independently of the provider evidence limit.

On Accepted, rewind the spool and read exactly the recorded number of length-prefixed items; forward one item at a time, including to a client with an in-flight load. On Discarded or a distinct Started boundary, drop the spool and construct a fresh one. If writing, reading, allocation or either budget fails, cancel the leader and close its client connections without releasing provisional content. Existing durable replay is the recovery authority. Never disconnect only the last observer: the normal detach path would evict the session while its attempt is active.

The spool bounds leader-owned candidate retention; it is not a bound on all process allocations, client output queues or a terminal emulator's memory.

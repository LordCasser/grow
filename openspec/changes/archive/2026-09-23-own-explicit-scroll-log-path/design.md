## Context

`ScrollLogRecorder` lazily opens its target on the first serialized record. `open_writer` currently creates/opens with `O_NONBLOCK`, rejects non-regular files, and then truncates. Each process owns an independent file offset, so checking only file type does not protect an existing file from a second recorder.

## Decision

Use the workspace's `fs2` advisory file-lock API on the opened file handle. Open without truncation, validate the target as a regular file, acquire a nonblocking exclusive lock, then truncate. Keep the locked `File` inside the existing `BufWriter` for the recorder's lifetime; dropping the sink releases the OS lock. This coordinates path aliases that resolve to the same inode and avoids sidecar lock files or stale ownership after a process crash.

A lock conflict is an ordinary open failure for the recorder: it follows the existing single-warning, disable-on-I/O-error behavior. The open flags and byte accounting remain unchanged.

## Risks

Advisory locks coordinate cooperating recorders. External programs that ignore the lock can still write to the target. Filesystems that do not support advisory locking cause recording to disable with a warning. Existing special-file handling remains before any truncation.

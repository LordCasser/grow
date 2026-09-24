## Boundary

`trim_file` opens the same path and holds an exclusive advisory lock for its in-place read, rewrite and truncate. `LogWriter` owns a persistent `O_APPEND` descriptor. Its process-local `WRITER` mutex only serializes calls within one process; it cannot order a sibling process's trim against an append.

## Decision

After `maintain`, the writer acquires the descriptor's exclusive advisory lock, writes the complete encoded entry, then unlocks. A failed lock or write follows the existing best-effort warning/drop path. The trim path and its `try_lock` policy remain unchanged: a trim encountering an active append yields, and an append encountering an active trim waits for that bounded tail rewrite before writing. No second lock file or queue is introduced.

The lock is on the opened inode rather than the path, matching the in-place trim contract. Path replacement races between maintenance ticks and slow filesystem latency are separate existing boundaries.

## Validation

Hold the trim lock, start a real writer append, rewrite/truncate the held inode, then release the lock. The append must complete only after release and remain after the retained tail. Run diagnostics tests and OpenSpec validation.

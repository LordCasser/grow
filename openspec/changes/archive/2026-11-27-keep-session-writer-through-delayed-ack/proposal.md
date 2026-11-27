## Why

A slow resumed delegate can occupy the embedded Shell thread that services the parent persistence queue. The independent parent ChatState actor treats 30 seconds without an answer to an ordinary journal append as a dead writer. Shell then retires the resident root. A queued operation with an expired deadline can also retire a healthy writer before submitting any I/O.

## What Changes

Keep ordinary in-flight writes owned by the original actor until the persistence worker replies or the actor is cancelled. Reject an already-expired, undispatched operation without reserving the next sequence. Preserve bounded retries for reported failures, explicit persistence barriers and fail-closed handling of permanent failures.

## Impact

ChatState journal admission and its Shell consumers. Journal records and caller APIs are unchanged.

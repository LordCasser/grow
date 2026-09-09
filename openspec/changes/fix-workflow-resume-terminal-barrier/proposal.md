## Why
Workflow watcher publishes a resumable tracker state before its terminal manifest and Timeline Ended acknowledgment. Resume currently reads that transient state without awaiting the existing terminal barrier, unlike pause and cancel. A resume can overtake the Ended event and fail causal validation.

## What Changes
Require resume of a settling execution to await its terminal acknowledgment before admitting a new execution epoch; reject still-active runs as before. Reuse the existing barrier, preserve completed journal replay and immutable run source.

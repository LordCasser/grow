## Why
SessionThread termination does not prove the writer lease has been released. The persistence task is spawned on the caller's Tokio runtime, while the session actor lives on another OS thread. Close/drain currently joins only that thread. A subsequent same-process cold load can race persistence task teardown and report an active writer.

## What Changes
Include the actual persistence owner termination in the existing session lifecycle drain boundary. Keep replacement admission closed until both owners have exited, without arbitrary sleeps, lease-conflict retries or bypassing exclusive locks.

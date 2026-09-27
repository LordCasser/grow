## Context

The permission actor serializes writes with a mutex and takes the latest state snapshot after acquiring it. Reset already creates a separate writer task, so external I/O does not block the command loop. The one-shot ACK is currently sent before that writer runs. Lower-level persistence helpers discard `io::Error` after logging.

## Decision

Make the persistence helper return `io::Result<()>`. The Reset writer sends the one-shot result after attempting the latest-state write. The public handle awaits it and returns a concrete error; `AllowAll` is a no-op success. Shell's reset notification path emits a durable UI-only error notice on failure and logs the failure. The memory cancellation and state replacement still occur synchronously in the actor before the writer begins. Other permission writes retain their existing nonterminal logging behavior.

No new actor or persistence queue is added. Success means the existing atomic write operation returned successfully. Failure means the current process remains reset in memory, but the caller must not claim durable completion.

## Verification

Use a writable temporary state directory for success and an unwritable/invalid parent path for a controlled write failure. Verify the handle result and no old grant remains on disk after success; check the Shell UI notice branch and permission suites.

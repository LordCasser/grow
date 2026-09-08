## Why
TUI waits for log flush before terminal restoration and agent cleanup; headless also awaits it during exit. acp_send waits indefinitely if a peer retains its response sender without acknowledging. Optional diagnostic delivery must not indefinitely hold shutdown.

## What Changes
Limit the current buffered-batch delivery wait to two seconds. Preserve prompt completion on acknowledgement/disconnection and existing no-owner retention. Do not add retries, join historical detached batches or claim transport cancellation.

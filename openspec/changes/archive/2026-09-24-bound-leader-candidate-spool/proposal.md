## Why

After independent notifications leave the leader's retractable candidate buffer, provisional ACP payloads can still grow with a long provider response. The 64 MiB raw-response evidence limit does not directly bound their serialized envelope size or fragment count. The measured RSS workload is not a hard ceiling.

## What Changes

Keep a small in-memory candidate spool, spill later provisional records to a temporary file, and stop the leader safely if a candidate exceeds a finite on-disk byte or record limit or the spool fails. Replay the accepted candidate one record at a time to non-retracting observers. Discarded candidates remain private and their spool is deleted.

The candidate is still transient presentation state. Canonical response admission and durable session replay stay with the session actor.

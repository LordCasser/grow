## Ownership

A process-wide `Forwarder` owns the pre-init and live queue. The first `init` owns one Tokio worker and ACP sender for its lifetime. Producers from any thread serialize into a counting writer before admission; at most 64 KiB per entry and 1 MiB / 256 pending entries are retained. Full queues drop the incoming entry rather than displacing earlier evidence. At most 16 entries and 256 KiB are removed into one in-flight batch; the worker awaits its ACP acknowledgement with a two-second bound before taking the next batch. Thus the queue plus one batch is bounded even when the peer stalls.

## Exit frontier

Each accepted entry receives a monotonic sequence. After a batch succeeds, fails or times out, the worker publishes its last settled sequence. `flush_blocking` captures the latest accepted sequence, wakes the worker and waits on that frontier for at most two seconds. It covers periodic and earlier count-triggered batches already removed from the queue. A timeout is a failed best-effort delivery boundary, not a claim that the remote writer persisted a log.

## Checks

Unit tests pin pre-init and single-entry budgets, FIFO delivery, delayed earlier-batch barrier and timeout/closed-peer behavior. Focused Pager tests, type check and OpenSpec validation precede archive.

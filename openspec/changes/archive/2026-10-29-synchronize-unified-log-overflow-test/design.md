## Design

Keep `append_queued_record` and the writer loop unchanged. The existing blocked-writer case first flushes the first record through a queue barrier, then locks the writer, queues two later records, and overflows the one-slot channel once. The loss diagnostic is therefore constrained to after the first record and before the final queued record; it may precede or follow the middle record depending on whether that record swapped the counter before blocking on the lock.

A second controlled case fills the one-slot queue and records exactly one rejected enqueue before starting the worker. Because the drop count is established before the worker can process the first record, the diagnostic must precede that first complete record. Together, the fixtures cover both legal sides of the counter-swap boundary without adding production hooks or weakening record ordering/drop-count assertions.

The adjacent debug firehose overflow test checks that a loss record is present but does not assert its position relative to a particular data record, so it has no equivalent fixed-index assumption and remains unchanged.

Root review removed the blocked-case intermediate atomic assertion: the worker can legally swap the drop counter into its pending diagnostic before taking the writer lock. Final exact diagnostic count remains authoritative. Each scoped closure owns its sender so assertion unwind closes the queue before scope joins, avoiding a test-only deadlock.

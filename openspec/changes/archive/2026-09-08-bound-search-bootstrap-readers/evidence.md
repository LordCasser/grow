# Audit evidence

Current source reindex_all collects Vec<(Summary, TimelineLedgerReader)> before constructing its semaphore. TimelineLedgerReader owns std::fs::File. JsonlStorageAdapter opens a regular identity-bound Timeline file. The initial collection propagates any open failure, so descriptor exhaustion aborts bootstrap before spawning any indexing task.

A separate Python subprocess with 96 temporary files and RLIMIT_NOFILE soft=64 reproduced the eager-open resource pattern: errno 24 after 61 retained files; four-file batches opened all 96. This is a resource-pattern experiment, not yet a Rust bootstrap regression. No user data was read or written.

Atlas scoped search returned a stale reindex_all signature and line despite reporting complete scoped coverage. Current direct source, not that stale record, supplies the ownership/control-flow evidence.

Follow-up source evidence: open_timeline_reader calls open_session, whose cache miss inserts Arc<ContainedDirectory> into opened_sessions. Thus bounding ledger files alone is insufficient: search also pins a directory per visited session. Both ownership paths belong to this same descriptor-budget defect.

Actual Rust red regression: bootstrap_many_sessions_under_descriptor_limit failed (0 passed / 1 failed, parent 3.06s, child 3.02s) with OS error 24 Too many open files at bootstrap_with_lease. The child initialized 96 actual temporary sessions, dropped their writer adapters, reduced only its own RLIMIT_NOFILE to 64 and called the production bootstrap entry.

# Verification

- `cargo check -p shell --tests`: passed.
- `cargo test -p shell leader::server::tests:: --lib`: 133 passed, 0 failed. The mixed-client scenario sends an approximately 8 MiB candidate through the real leader route, crossing the spool's memory threshold before Accepted; the observer receives it in order only after acceptance. The same test checks independent notifications during that interval and candidate omission after Discarded. Existing load-overlap routing remains covered by `accepted_sampling_candidate_flushes_pending_load_once` in the same suite.
- `candidate_spool_spills_in_order_and_rejects_limit_without_writing` verifies disk rollover, FIFO readback, exact byte ceiling and record ceiling without allocating a 512 MiB fixture.
- `cargo fmt -p shell`, `git diff --check`, targeted strict OpenSpec validation and full strict OpenSpec validation passed.
- A spool failure before admission shuts down the leader transport and never flushes provisional candidate records to non-retracting observers. This is a bounded fail-closed presentation path; it is not a process-wide RSS ceiling or a guarantee about terminal memory.

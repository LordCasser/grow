## Verification

- CI evidence `/tmp/grow-2.5.2-ci-core-job.log` reproduced the assertion twice: `append_queued_record` emitted the `records_dropped` line before `first` because the drop counter was incremented after queue dequeue but before the worker's counter swap. The root also reported the same failure in the independent `pushcore-failed.log` job.
- Source trace: `append_queued_record` swaps the counter before acquiring the writer mutex. Queue removal therefore does not establish that the worker has snapshotted the counter. The blocked-writer fixture now uses `flush_sender` to commit `first` before triggering overflow; its diagnostic is constrained to the interval after `first` and before `third`, with `second` retained in order and exactly one dropped record.
- Added `overflow_before_first_append_reports_loss_before_first_record`, which fills the one-record queue and records one nonblocking drop before starting `writer_loop`; the expected diagnostic-before-first ordering is deterministic.
- Reviewed `debug_log::stream_firehose::tests::blocked_disk_overflows_queue_without_blocking_producers_and_reports_loss`: it checks for a loss record but makes no fixed-position assertion, so no adjacent test change is needed.
- `rustfmt --edition 2024 --check crates/codegen/diagnostics/src/unified_log.rs`: passed.
- `git diff --check`: passed.
- `openspec validate --all --strict --no-interactive`: passed in this task's validation run.
- Cargo tests were not run here because the root task owns the coordinated Cargo job; the diagnostics test result remains pending.

Root最终验证：`cargo test --locked -p diagnostics --lib -- --test-threads=4` 55 passed / 0 failed；同一最终编译产物对两个精确溢出测试各运行100次，共200次执行通过，每次独立进程10s期限（非生产重试）。日志 `/tmp/grow-2.5.3-diagnostics-final.log`、`/tmp/grow-2.5.3-diagnostics-stress.log`。`cargo fmt --all --check`与源码/文档diff检查通过。

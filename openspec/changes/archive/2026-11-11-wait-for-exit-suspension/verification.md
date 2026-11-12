# Verification

macOS arm64, Rust 1.93.1, locked and offline builds.

- `cargo test --offline --locked -p pager -p pager-minimal --lib exit` passed 81 Pager tests and 2 minimal tests. The Pager ACP effect kept its original responder live for six simulated seconds and accepted its late confirmation. Tests also exercised repeated Ctrl+D, explicit force during saving, retry after failure, stale responses, and narrow rendering.
- `cargo test --offline --locked -p shell --lib suspension_admission_tests` passed all 6 tests. The original ACP request waited past six simulated seconds for the lifecycle lock and for a nonresident physical writer. Separate scopes and retries after failed acknowledgement retained their original behavior.
- The same Shell test executable passed the late compaction acknowledgement and bounded compaction retry tests, the 3 `restore_retry_tests` cases, and the 3 `suspension_auxiliary_admission_tests` cases. The late acknowledgement test verifies the original node suspension reaches its durable checkpoint without synthesizing a Turn terminal.
- The existing `complex_history_exit_through_acp` test passed on 305,653,152 bytes and 2,494 source events. The ACP request confirmed suspension after 158 ms, the original journal prefix matched, writers retired, and cold validation reopened the completed tree. The six-second tests above cover slow acknowledgements. The synthetic history test covers actual storage and retirement.
- Built `cargo build --offline --locked -p cli --bin grow`. The `pty_e2e_persistence` test `ctrl_d_suspends_running_root_and_continue_resumes_without_replay` passed using that binary. The first process exited after two Ctrl+D presses during a running command. A new process resumed the saved work without replaying that command.

- The same PTY suite passed `ctrl_d_suspends_and_cold_resumes_root_child_grandchild_tree` against the same build, restoring the original nested identities and work.

No session at the reported 1000M-token scale was used.

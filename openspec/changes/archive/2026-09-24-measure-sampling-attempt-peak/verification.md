# Verification

- macOS built-binary benchmark: `cargo test -p shell --test test_built_binary_e2e test_headless_sampling_attempt_peak_rss -- --ignored --nocapture` passed.
- Final rebuilt `grow` binary, small SSE foreground response: maximum resident set size 123,469,824 bytes (117.8 MiB).
- Final rebuilt `grow` binary, approximately 12 MiB SSE foreground response in ~16 KiB chunks: maximum resident set size 255,967,232 bytes (244.1 MiB).
- Observed peak difference: 132,497,408 bytes (126.4 MiB), about 10.5 times the response text size. This is a process-level peak, not a hard memory ceiling or an isolated heap attribution. The mock server runs outside the measured child process; automatic title sampling uses a separate short-response server.
- Both runs exited successfully and made one foreground Chat Completions attempt to the measured endpoint.
- The remaining attempt-period Grow persistence/gateway paths were reviewed and closed by `2026-09-24-bound-aux-grow-notification-queues`, `2026-09-24-bound-subagent-lifecycle-projection`, and `2026-09-24-bound-tool-bridge-grow-delivery`. Their validation records are in the archive. The preview-memory backlog entry is removed.

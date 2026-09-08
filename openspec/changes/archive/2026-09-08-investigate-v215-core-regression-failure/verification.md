# Verification
Pending; no completion inferred from authorization.

Run: https://github.com/LordCasser/grow/actions/runs/34213624575
Commit: 03eb765aa2de211796b49023449f7db0fa5a3562
Before shell aborted, completed groups reported 464, 302, 7160 (10 ignored), 86, 220 and 264 passing tests. Shell builtin tests failed_extraction_does_not_publish_marker_and_retries, same_version_reconciles_managed_files_without_touching_user_skills and version_bump_reextracts_managed_files_without_touching_user_content failed. The process later overflowed an unnamed thread stack, preventing final assertion diagnostics. Raw log saved outside repository at /tmp/grow-v215-core-failure.log. No release workflow dispatched.

R32 local default-parallel Sampler regression: 217/218 passed; sampling_auth_logs_omit_credentials failed because captured logs omitted sampling_request while client_post was present. Unmodified log test passed in the serial full run (218/218). Investigate tracing callsite/subscriber interactions separately; do not weaken credential assertions or attribute this to callback removal without evidence.

Builtin root cause reproduced on Ubuntu noble arm64 with cap-std 4.0.2 and unchanged transaction code: O_PATH descriptor fsync fails with EBADF. Fix and regressions are tracked separately in fix-builtin-directory-sync.

Overflow reproduced locally with only async_compaction_authority_transition_cancels_before_publication running on its unchanged 8 MiB thread. LLDB catches the production process_conversation_turn poll stack probe (0x24b6f0-byte frame) beneath embedded recovery/admission futures. Allocation experiments did not solve it and were reverted. Unoptimized test-frame capacity correction is tracked in fix-unoptimized-session-test-stack.

Full Linux core CI passed at f47190ca: https://github.com/LordCasser/grow/actions/runs/34225938787 . ChatState 464, memory 302, Pager 7154 (10 ignored), pager-minimal 86, Sampler 218, sampling-types 264, Shell 3754 (3 ignored), Workflow 65, and the CLI build completed. No stack overflow, directory EBADF or credential-log capture failure occurred. This validates the pre-R27-feature baseline; the image fallback feature will receive a separate final CI run.

# Verification
Pending; no completion inferred from authorization.

Run: https://github.com/LordCasser/grow/actions/runs/34213624575
Commit: 03eb765aa2de211796b49023449f7db0fa5a3562
Before shell aborted, completed groups reported 464, 302, 7160 (10 ignored), 86, 220 and 264 passing tests. Shell builtin tests failed_extraction_does_not_publish_marker_and_retries, same_version_reconciles_managed_files_without_touching_user_skills and version_bump_reextracts_managed_files_without_touching_user_content failed. The process later overflowed an unnamed thread stack, preventing final assertion diagnostics. Raw log saved outside repository at /tmp/grow-v215-core-failure.log. No release workflow dispatched.

R32 local default-parallel Sampler regression: 217/218 passed; sampling_auth_logs_omit_credentials failed because captured logs omitted sampling_request while client_post was present. Unmodified log test passed in the serial full run (218/218). Investigate tracing callsite/subscriber interactions separately; do not weaken credential assertions or attribute this to callback removal without evidence.

Builtin root cause reproduced on Ubuntu noble arm64 with cap-std 4.0.2 and unchanged transaction code: O_PATH descriptor fsync fails with EBADF. Fix and regressions are tracked separately in fix-builtin-directory-sync.

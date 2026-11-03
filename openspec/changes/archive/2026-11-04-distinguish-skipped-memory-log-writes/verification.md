# Verification

| Area | Result | Evidence and boundary |
| --- | --- | --- |
| Memory storage | 302 passed / 0 failed | `memory` crate result in `/tmp/grow-260-final-core5.log`; covers the complete crate library suite. |
| Daily-log storage subset | 83 passed / 0 failed | `/tmp/grow-260-memory-storage.log`; storage-focused subset, including ephemeral workspace suppression and written/skipped storage behavior. |
| Flush checkpoint receipt | Passed | `session::actor::tests::image_input_recovery_tests::auxiliary::ephemeral_daily_log_skip_does_not_checkpoint_flush_as_written` is `ok` in final Shell run `/tmp/grow-260-final-core5.log`; the focused run `/tmp/grow-260-shell7-target-3.log` also passed 1/1. |
| Session-end receipt | Passed | `session::memory::hooks::tests::test_on_session_end_reports_ephemeral_storage_skip` is `ok` in `/tmp/grow-260-final-core5.log`; focused run `/tmp/grow-260-shell8-target-1.log` passed 1/1. |

The Shell crate finished with 4576 passed / 0 failed / 9 ignored in the final core run. These tests establish that an ephemeral policy skip is reported as skipped and does not count as a successful flush checkpoint; they do not change the existing ephemeral-write policy. Candidate-wide core results are 19233 passed / 0 failed / 51 ignored across 18 crate blocks (`/tmp/grow-260-final-core5.log`); workspace check passed in 1m30s with no warnings, Settings E2E passed 235/235, and strict pre-archive validation passed 26/26 (`/tmp/grow-260-final-strict-prearchive.log`). Ordinary local release build passed in 3m21s; it is not an official distribution build.

Archival, native cross-platform CI, PTY gates and official publication remain separate and incomplete. Earlier targeted evidence and release-blocker chronology remain in the [umbrella verification](../../close-v2-6-0-release-blockers/verification.md).

## 归档收尾（2026-11-04）

已执行 `openspec archive distinguish-skipped-memory-log-writes --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../../close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。

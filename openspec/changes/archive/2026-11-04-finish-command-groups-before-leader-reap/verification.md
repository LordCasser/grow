# Verification snapshot

| Test scope | Result | Evidence and boundary |
| --- | --- | --- |
| Unix TERM-ignoring descendant | Passed | `/tmp/grow-260-tools7-target-1.log`: 1 passed / 0 failed. Final core run `/tmp/grow-260-final-core5.log` also reports `command_cleanup_finishes_term_ignoring_descendants_before_leader_reap` as `ok`. |
| Terminal library group | 57 passed / 0 failed / 5 ignored | `/tmp/grow-260-tools-target-0.log`; terminal-focused group. |
| Full Tools library | 2441 passed / 0 failed / 7 ignored | Final `/tmp/grow-260-final-core5.log`; includes the terminal cleanup regression. |
| Task cancellation regression | 21 passed / 0 failed | `/tmp/grow-260-final-target-implementations::grow_build::kill_task5.log`; adjacent entry-scope coverage, not a substitute for terminal cleanup behavior. |

This is macOS local evidence for descendants that remain in the command process group. It does not prove PID 1 reaping, cover descendants that detach with `setsid`, or close the pre-existing `ProcessScope` upgrade/reap concurrency window described in [review.md](review.md). Candidate-wide core results are 19233 passed / 0 failed / 51 ignored across 18 crate blocks; workspace check passed in 1m30s without warnings, Settings E2E passed 235/235, and strict pre-archive validation passed 26/26. Ordinary local release build passed in 3m21s but is not an official distribution build. Other platform behavior requires native CI; archival and official publication remain incomplete. Earlier failures and investigation history remain in the [umbrella verification](../2026-11-04-close-v2-6-0-release-blockers/verification.md).

## 归档收尾（2026-11-04）

已执行 `openspec archive finish-command-groups-before-leader-reap --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../2026-11-04-close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。

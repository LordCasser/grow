# Verification

## Responses PTY cases

| Case | Result | Evidence |
| --- | --- | --- |
| Parallel tool calls | 1 passed / 0 failed, 6.57s | `/tmp/grow-260-responses-parallel-final.log`; exact `edit_merge_parallel_pty::edit_merge_parallel_pty` case compiled and ran on the final CLI. |
| Single tool call | 1 passed / 0 failed, 8.06s | `/tmp/grow-260-responses-single-final.log`; exact `verb_group_fold_expand_collapse_pty::verb_group_fold_expand_collapse_pty` case compiled and ran on the final CLI. |
| Existing Core gate wiring | Present | `.github/workflows/core-regression.yml:140-141` invokes both exact ignored PTY cases through `scripts/run_exact_cargo_test.py`. |

The parallel case uses a foreground-specific response expectation so sideband traffic cannot consume the scripted foreground Responses response. It still asserts the HTTP tool calls, two `function_call_output` items, both edited file markers, and the single merged edit row. The fixture creates a real Git repository under its temporary project directory, matching the CLI's workspace classification rule.

## Other final local gates

| Gate | Result | Evidence |
| --- | --- | --- |
| Protocol PTY recovery cases | 3 × 1 passed / 0 failed | `/tmp/grow-260-final-goal_sampling_failure_requires_explicit_restart_and_preserves_budget_gates8.log`, `/tmp/grow-260-final-malformed_wire_json_allows_manual_continuation_in_the_same_resident_session8.log`, and `/tmp/grow-260-final-protocol_failure_preserves_session_across_continue_switch_and_process_restart8.log`. |
| Workspace check | PASS, 1m30s, no warnings | `/tmp/grow-260-final-workspace8.log`. |
| Formatting check | PASS | `/tmp/grow-260-final-format.log` (empty output, successful check). |
| Settings E2E | 235 passed / 0 failed | `/tmp/grow-260-final-settings8.log`. |
| Final strict OpenSpec validation | 26 passed / 0 failed | `/tmp/grow-260-final-strict-prearchive.log`. This is pre-archive validation. |
| Ordinary local release build | PASS, 3m21s | `/tmp/grow-260-final-release8.log`; this was a local `release` build, not an official distribution build. |
| Core library run | 19233 passed / 0 failed / 51 ignored across 18 crate blocks | `/tmp/grow-260-final-core5.log`. |

The local binary `target/release/grow` is Mach-O arm64, reports `grow 2.6.0 (bbd2c4dfa) [alpha]`, passes strict code-signature verification, and has SHA-256 `31348dd8394ffa801197767fc3947d44dc44ff5c23a07a8ba3ae53f3128c4342`. It was built from an uncommitted source tree and is not an official distribution asset. Native CI and publication remain separate gates.

## 归档收尾（2026-11-04）

已执行 `openspec archive fix-pty-responses-tool-stream-fixtures --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../2026-11-04-close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。

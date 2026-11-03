# 验证记录

## 已完成的本机验证

| 范围 | 结果 | 证据与边界 |
| --- | --- | --- |
| Disposable-copy interruption 协议 | PASS | `python3 openspec/changes/analyze-command-resource-budget-and-cleanup/validate_isolated_interruption.py`；`/tmp/grow-260-isolated-interruption.log` 显示 trial 被中断、baseline 未变、临时副本已删除。只验证隔离与清理协议，不验证产品资源硬限额。 |
| Production descendant cleanup regression | PASS | 最终核心运行 `/tmp/grow-260-final-core5.log` 中 `command_cleanup_finishes_term_ignoring_descendants_before_leader_reap` 为 `ok`；Tools 全 crate 为 2441 passed / 0 failed / 7 ignored。另有 `/tmp/grow-260-tools7-target-1.log` 的 bounded descendant 目标测试 1/1 通过。实现细节和 PID/PGID 并发边界见[命令组 cleanup verification](../2026-11-04-finish-command-groups-before-leader-reap/verification.md)。 |

## 结论边界

本次调查未增加固定内存预算、CPU/进程配额或全局运行开关。高资源试验预算仍须由调用方/宿主显式给定；上述临时副本脚本是低资源中断协议验证，不是高负载试验。早期失控 probe 未运行且已删除；先前测试失败和修复过程保留在[发布阻塞总表](../2026-11-04-close-v2-6-0-release-blockers/verification.md)。

候选版最终本机核心测试在 `/tmp/grow-260-final-core5.log` 汇总为 18 个 crate、19233 passed / 0 failed / 51 ignored；workspace check 为 PASS（1m30s，无 warning），Settings E2E 为 235 passed，strict pre-archive 校验为 26 passed / 0 failed。普通本地 release build 通过（3m21s），但不是 official distribution build。macOS 本机结果不代表 Linux/Windows native CI；归档与正式发布仍未完成。

## 归档收尾（2026-11-04）

已执行 `openspec archive analyze-command-resource-budget-and-cleanup --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../2026-11-04-close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。

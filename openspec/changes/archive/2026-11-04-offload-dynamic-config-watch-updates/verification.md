# 验证记录

## 最终本机证据

| 范围 | 结果 | 证据与边界 |
| --- | --- | --- |
| ProjectDiscovery startup owner | 4 passed / 0 failed | `/tmp/grow-260-final-target-project_discovery_startup_tests5.log`；包含注册阻塞时 command ACK、refresh/Drop 线程归属、session runtime 与 writer ACK 先于 native start 返回、host runtime shutdown guardian。writer ACK 场景使用受控 persistence receiver；不替代真实 writer retirement。 |
| Config watcher | 20 passed / 0 failed | 20 个 `config::watcher::*` 用例见最终 Shell 全量运行 `/tmp/grow-260-final-core5.log`；覆盖目录创建/重建、alias、来源释放、同时间戳原子替换、排队顺序及 owner cancellation。 |
| Pager MCP hot reload | 14 passed / 0 failed | `/tmp/grow-260-pager7-target-0.log`，MCP 热更新目标组。 |
| 冷恢复与 session retirement | 全部通过 | 最终 Shell 全量运行 `/tmp/grow-260-final-core5.log`：birth-authority 冷恢复、durable effort cold resume、project-rules cold restore、task-resume transcript 用例均为 `ok`；Shell package 4576 passed / 0 failed / 9 ignored。 |
| Pager 全量单测 | 7566 passed / 0 failed / 15 ignored | `/tmp/grow-260-final-core5.log`。 |

## 限制

最终核心运行在 18 个 crate block 汇总为 19233 passed / 0 failed / 51 ignored。候选版 workspace check PASS（1m30s，无 warning），Settings E2E 235 passed，strict pre-archive validation 26 passed / 0 failed，普通本地 release build PASS（3m21s）。本机结果不代表 native cross-platform CI 或官方分发/发布；归档及这些剩余门禁仍未完成。早期 watcher/MCP 失败和中途修复记录保留在[发布阻塞总表](../../close-v2-6-0-release-blockers/verification.md)中，最终结果只替代相同本机门禁的旧结论。

## 归档收尾（2026-11-04）

已执行 `openspec archive offload-dynamic-config-watch-updates --yes`，相应 delta 已合入主规范；纯维护/调查 change 按 skip_specs 归档。最终本机结果见上表，跨平台 CI 与官方发布仍由[发布闭合记录](../../close-v2-6-0-release-blockers/verification.md)跟踪。归档后的严格校验日志为 `/tmp/grow-260-postarchive-active-final.log` 和 `/tmp/grow-260-postarchive-archived-final.log`。

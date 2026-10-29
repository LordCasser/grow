# 验证记录

2026-10-29，本地 macOS arm64、Rust 1.93.1。

- 真实 child 文件包含合法 Seed 但无 Result；backend 的 CompletedOutputUnavailable 原先合成 Completed/parent terminal，失败证据 `/tmp/grow-backlog-red2.log`（新增 seq2 完成事实）。
- 现有 canonical Result 在既有身份、writer/prefix/Seed 校验后优先；已合法完成但输出缺失或损坏仍保留 canonical outcome。没有 Result 且不可验证输出时 fallback 为 None，返回 Unpublished，parent spawn 保持未闭合。
- 正常 Completed 仍允许既有 fallback；Suspended/FinalizationPending 原授权门槛保持。
- 子会话定向测试 168 项通过；最终 Shell library 4413 passed / 9 ignored，日志 `/tmp/grow-2.5.2-shell-last.log`。真实文件覆盖不等同线上 backend 故障或整机断电。
- 开发说明与规范同步；同一发布记录保留最终全量校验与归档证据。

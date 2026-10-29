# 验证记录

2026-10-29，本地 macOS arm64、Rust 1.93.1。

- 先经真实 Session actor 的 PlanHandoff-origin Turn/active notification drain 构造同 Turn 重复核对：原实现误报 AcknowledgementLost，失败日志 `/tmp/grow-backlog-plan-red.log`。
- 修复后仅精确 source/hash/revision/kind/submission 且消费 Turn 为当前 Turn 才幂等成功；重复核对不新增 Timeline 事件。其他 Turn、Dismissed、owner 不可用维持拒绝。
- Timeline 的 receipt 索引及后缀查询覆盖 replay 与精确版本，不复制全表、不建立第二账本。
- Plan 定向集 130 项通过；最终 Shell library 4413 passed / 9 ignored，ChatState 全量 602 passed / 3 ignored。日志 `/tmp/grow-2.5.2-shell-last.log`、`/tmp/grow-2.5.2-core-final.log`（后者其他包初轮失败已分别修复复跑，不能作为全命令成功记录）。
- 开发说明同步至 `docs/development.md`；全量规范严格校验与归档结果由本轮发布记录统一记录。

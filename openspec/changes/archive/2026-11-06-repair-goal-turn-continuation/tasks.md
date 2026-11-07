## Tasks — repair-goal-turn-continuation（诊断 → 单一 run-loop wake 修复）

诊断轮（第 1–2 轮）：

- [x] 读 openspec 索引、behavior-goal / session-timeline / input-admission 相关契约要点。
- [x] 静态闭合 Active Goal 普通 turn 结束后自动续跑链路（settlement → handle_turn_end → idle arbiter → schedule/drive/start）。
- [x] 核对 blocker/usage/provider budget/pending input/pending step control/goal-internal 终止分支的 allow/block 有效性。
- [x] 第 2 轮收敛：撤销 F1"goal_drive 永久占位"断言；F2 限定为 idle 路径缺解除唤醒，唯一候选可达链 C1。
- [x] 与 late-ask 调查给出修订版交集不变量 INV-1。
- [x] 只读抽查当前主 session timeline：无真实 Goal projection/continuation 失败实证。
- [x] 修正 `.openspec.yaml` 为仓库合法格式。

修复轮（父冻结行为 delta 后实施，2026-11-06）：

- [x] T-C1 fixture：真实 `RunCoordinationInquiry` 入口 + cross-cwd Peer 审批 guard + 已启动 mailbox 后持久写入的 whole-work Permission wait + cancellation 解除；含 Stopped Goal 控制与 same-session ask 反证。
- [x] old-red：判别测试于唤醒超时失败（前提成立，missing-wake 证实）；两次前期 premise 失败（断言顺序 / goal tools 未注册）为 fixture 修正，记录保留于 `/tmp/grow-goal-interaction-old.log` 历史。
- [x] 最小 fix：`run_session` 订阅 `pending_interactions.subscribe_changes()`，select changed arm 经既有 `arbitrate_idle_wake` 响应；无新轮询/task/registry/多点 notify。
- [x] green：`--lib goal_interaction_wake` 3/3 passed。
- [x] 回归：`--lib goal` 201 passed、`--lib planning` 115 passed、0 failed。
- [x] 父核实现、实际 old-red/green 及独立 review，完成开发说明、strict validate 和归档。

## Proposal — repair-goal-turn-continuation（诊断 → 单一 run-loop wake 修复）

### Why

用户报告"当前 Goal 的 turn 自动接续好像也存在 bug"。第 1–2 轮为只读调查：Active Goal 普通 turn 结束后的自动唤醒主链静态闭合，F1（durable ACK → goal_drive 占位）撤销，F2 收敛为"idle 路径人问依赖解除不唤醒 idle arbiter"的窄缺口，唯一候选可达链 C1（外部 primary session 跨工作区 Peer inquiry 在目标 session registry 注册 Permission guard、目标 idle、解除无 notify）。用户原始 stall 现场仍未复现（本 change 不据此宣称已修用户报告）。

### What Changes

一处 run-loop 行为修复（delta spec 见 `specs/behavior-goal/spec.md`，不再 skip_specs）：

- `run_session` 初始化时订阅既有 `PendingInteractions::subscribe_changes()` watch（先订阅后处理命令，避免漏检），select 新增 changed arm，变更经**既有** `arbitrate_idle_wake` 顺序（step controls / compaction / FIFO / notifications / Goal）重新裁决 idle 工作。
- 不新增轮询、后台 task、registry 或多处 notify；turn 内 `await_planned_dependencies` 的既有 watch 订阅不动；busy 不夺 foreground；终止 fence 与 Stopped Goal 沿既有仲裁拒绝。
- 回归：`session/actor/tests/goal_interaction_wake_tests.rs`——真实 `SessionCommand::RunCoordinationInquiry` 入口 + cross-cwd Peer 审批 guard（gateway stub 持住 reverse request 模拟用户未决定）+ 已启动 mailbox 后持久写入的 whole-work Permission wait + inquiry cancellation 解除，判别"解除后无额外输入是否自动 Goal 续跑"；含 Stopped Goal 控制与 same-session ask 反证（guard 存活期间前台 busy，idle 前提不可达）。

### 已验证（2026-11-06，Cargo 窗口）

- old-red：判别测试在"registry resolution must wake idle Goal continuation"超时失败（前提全部成立：guard 注册、Goal parked、pending 期间 0 采样）——确认 missing-wake 缺口。此前两次 premise 失败（waits 顺序断言、goal tools 未注册）为测试侧 fixture 修正，已保留记录。
- green：修复后 3/3 passed（`--lib goal_interaction_wake`）。
- 回归：`--lib goal` 201 passed、`--lib planning` 115 passed，0 failed。

### Non-Goals

- 不修 late ask（另一 agent）；INV-1 交集不变量由此 wake 满足 idle 侧。
- 不宣称复现用户原始 stall：模型在 turn 内自行获得跨工作区 inquiry id 的可见性未证，测试 wait 使用已知 id 作为运行时承诺。
- 不改 Goal framework、backlog、真实 session/Goal/config。

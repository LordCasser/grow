## Design — repair-goal-turn-continuation（诊断，第 2 轮收敛）

### 已静态闭合的链路（allow 路径）— 与第 1 轮一致，维持有效

1. Turn 结算：`run_loop.rs` completion arm（~L1449–1514）→ `post_turn_goal_degradation_plan`（`turn/settlement.rs:417`）→ `handle_completion` → 固定次序（step controls → compaction → running task → notification drain → 若 idle）→ `handle_turn_end`（`turn/mod.rs:700`）。
2. `handle_turn_end` → `idle_arbiter.notify_one()`（turn/mod.rs:713）；suppress 仅对 goal-internal `StationarityEnded`。
3. `arbitrate_idle_wake`（`idle_arbitration.rs:85`）→ idle 时 `schedule_goal_on_idle`（`goal.rs:61`，spawn 后兜底 re-notify）→ `drive_goal_on_idle`（goal.rs:782）→ `start_goal_internal_turn`（goal.rs:829）。

各 block 分支（Paused/Blocked/Complete、预算耗尽、usage_incomplete、用户 FIFO/held 头、goal-internal 终止、`GoalSpendingStopped`、终止 fence）核对结论同第 1 轮：与 behavior-goal spec 及既有测试一致。

### F1（goal_drive 卡占位）— 第 1 轮断言不成立，已收敛

第 1 轮断言"durable ACK 失败只 log → `goal_drive` 永久占位"**不成立**：

- `durable_start_rx.await != Ok(true)` 之后 `start_goal_internal_turn` **直接返回**，`drive_goal_on_idle` 随之返回，spawn 包装的 task 结束 → `goal_drive.is_running()==false`，无占位。
- 发送侧覆盖（`turn/admission.rs`）：`ensure_prefix_ready` 失败 → `signal_durable_turn_start(false)`（:316）后返回 Err；`start_turn` 持久化失败 → false（:469）后返回 Err；成功 → true（:478）。`ensure_prefix_ready`（`session_setup.rs:85`）的 10s 超时只包住 background join handle；超时后走 sync fallback `build_user_message_prefix().await`，该 fallback 无总 deadline，因此不能断言 `ensure_prefix_ready` 整体绝不会 park。该 park 属通用 prefix 构建故障面，仍无已证 Goal 特有 root。
- 发送者被 drop 而未 send（更早的 Err 返回、task abort）：oneshot rx 返回 `Err(RecvError)` ≠ `Ok(true)` → 同样返回，不挂起。
- 失败后续：durable start 失败 → `run_task` 返回 fatal turn-boundary Err → completion arm 对 goal-internal turn 走 `error_stop` 自动 pause —— 是既有降级设计，非卡死。
- 剩余未证边缘：仅当 `start_turn` 的 durable Timeline append 本身无限 park（writer 停摆）时 ACK 永不 settle；这是通用 Timeline 故障面，非 Goal 特有缺陷。**结论：F1 无确定缺陷，撤销"永久占位"断言。**

### F2（idle 期人问依赖解除无唤醒）— 限定为 idle 路径，可达性未证

修正第 1 轮两处过度表述：

1. "无任何 waiter"**仅适用于 idle 路径**：turn 内路径 `await_planned_dependencies`（planning.rs:1147 `subscribe_changes()` + :1253 select）对 Question/Permission 解答有 watch 唤醒；idle 路径 `planning_waits_for_event`（goal.rs:811 调用）是单次轮询、无订阅。
2. 就绪语义澄清（planning.rs:1064–1077）：Question/Permission 依赖在 `pending_interactions` 中存在匹配 active 请求 → `Ok(false)`（未就绪，阻塞 Goal）；已解除/不存在 → `Err` → `planning_waits_for_event` 的 `unwrap_or(true)` 视为 ready → wait 满足 → Goal 可续跑。即"解答/超时后下一次 idle wake 必能放行"，缺的只是**解除事实本身不 notify `idle_arbiter`**。

可达性分析（本 session 内 ask/permission 均为阻塞式 model tool batch）：

- 本 session 的 `ask_user_question`/permission：guard 存活期间 turn 不可能 idle；取消/超时/abort 时 guard drop → InteractionResolved，且随后 turn 终止 → `handle_turn_end` notify，顺序上先解除后唤醒 → 放行。**未找到现实 root 路径。**
- 候选链 C1（唯一找到的合法前提）：**外部 primary session 的跨工作区 inquiry**（`InquiryAuthority::Peer`、source_cwd ≠ target cwd，非子 Agent Delegation）。`coordination.rs:~861-925` `request_coordination_approval` 在 !same_cwd && !delegated 分支于**目标 session** 的 `pending_interactions` 注册 `PendingInteractionGuard`（Permission kind，id = inquiry_id），目标 foreground 可同时 idle。若存在 whole-work plan wait 以该 inquiry id 为 Permission 依赖，Goal 在 idle 被阻塞；inquiry 超时（`APPROVAL_TIMEOUT`，120s）/取消 → guard 解除 → 无 `idle_arbiter` notify → Goal 停摆至任意外部事件。前提苛刻（需 Goal turn 声明过匹配该 inquiry id 的 wait 且 turn 已结束），**未证明现实中可达成**。
- `YieldGoalForFifo` 发生在 turn 内 `await_planned_dependencies` 阶段，前提是 FIFO 已有用户输入——该输入的入队路径本身会 notify arbiter，解除后必有唤醒；未发现其与 idle stall 的直接链。Plan park（PlanApproval）kind 不同，未纳入本调查。

### 交集不变量（给 late-ask agent，不混修复）

**INV-1（修订）**：session idle 且 `goal_loop_active()` 为 true 且存在因 pending Question/Permission 而未就绪的 whole-work plan wait 时，该交互的解除事实（解答/超时/取消）必须触发 `idle_arbiter` 唤醒。当前实现不满足，但**现实可达路径仅 C1（子 Agent inquiry）候选，未证实**。

### 最小判别实验（建议，未执行；Cargo 属父 AC01 唯一窗口）

- **T-C1**：LocalSet actor：Active Goal + whole-work wait（Permission 依赖，id 预定）→ 置 idle → 通过**真实 coordination inquiry 入口**（非手工 set private state）注册同 id 的 pending Permission → 断言 Goal continuation 被阻塞 → 触发 inquiry 超时/取消 → 断言在无额外输入的情况下最终出现 `GoalContinuation` foreground（超时即证 C1）。fixture 方案：复用既有 coordination 测试的 inquiry 构造 + 不应答的 gateway stub + 虚拟时钟推进 `APPROVAL_TIMEOUT`。
- **T-F1（降级为回归防护，可选）**：durable start false/drop 注入 → 断言 `goal_drive` 立即可重 arm 且 Goal 按 error_stop 降级（验证既有设计而非修缺陷）。
- **日志判别（真实用户复现路径）**：用户 goal session 搜 `Goal continuation failed before durable turn admission`、`InteractionResolved`、`goal_continuation` Turn::Started 序列与 idle 间隔；Goal 仍 Active、无 pause/budget 事实、最后事件是 idle 期 InteractionResolved 且之后无续跑 → 命中 C1。

### 状态

主链闭合；F1 撤销（无缺陷）；F2 收敛为"idle 路径缺解除唤醒"的窄缺口，现实可达性仅剩 C1 候选，未证实。不宣称复现用户 bug；根因判定与实施由父负责。若 T-C1 证实：最小 owner fix = 在 idle-relevant 人问依赖解除的 durable 提交路径 notify `idle_arbiter`（一处，不扩 Goal framework）。

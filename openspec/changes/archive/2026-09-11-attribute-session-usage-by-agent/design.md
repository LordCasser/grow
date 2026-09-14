## Context

当前 `UsageLedger` 已拥有总体与 provider/model 分项。主循环调用直接写入所属 session 的账本；子 Agent 结束时读取其 session 累计快照，把 `by_model` 经 shell `SessionCommand::RecordSubagentUsage` 和 chat-state ACK 折叠进父 prompt/session 账本。该命令已经携带稳定的 `subagent_id`，但 `SessionActor::record_subagent_usage` 将它标为未使用，chat-state 也没有 Agent 维度。

因此现有直接账本测试可以证明子消费被加到总体，却不能保留来源；真实链路的身份在进入账本前丢失。现有 Usage 和 normal 状态栏都从 `UsageLedger.totals` 的 `PromptUsage` 投影读取，不需要新的展示模型。

## Goals / Non-Goals

**Goals:**

- 在唯一的进程内会话账本中保留主 Agent 与各 `subagent_id` 的已知消费。
- 让 Agent 分项、provider/model 分项和总体由同一次 fold 产生，避免平行计费路径。
- 用测试证明子 Agent 总量进入现有公共投影，但 Agent 分项当前不进入 wire/UI。

**Non-Goals:**

- 不在子 Agent 运行期间流式上报；仍沿用终态累计快照与既有 freeze/incomplete 边界。
- 不持久化跨进程 usage，不展示逐 Agent 明细，不改变 Goal/Sideband 账本所有权。
- 不修复本次范围外的子视图 Usage 路由债务。

## Decisions

1. `UsageLedger` 增加进程内 Agent 分项，以类型化 key 区分账本所属 Agent 和 `Subagent(String)`，避免保留字符串哨兵或与用户提供的 task id 冲突。分项复用 `UsageTotals`，不建立第二套 token 算法。
2. `record_main_loop_call` 在更新总体与 model 分项的同时更新所属 Agent；`record_subagent` 接受 `subagent_id`，对同一份 `by_model` 同步更新总体、model 与该子 Agent 分项。这样任意一项都不会单独成为计费权威。
3. shell 和 chat-state 的现有命令/ACK 链路透传 `subagent_id`。子 Agent 仍只在终态发送一次自身 session 累计快照；不增加中间事件、轮询或增量对账状态。
4. `PromptUsage` 明确忽略内部 Agent 分项，只从原有总体和 model 分项构建。由此 `/usage`、headless 和 normal 状态栏自然得到包含子 Agent 的总量，同时保持公共形状不变。

## Risks / Trade-offs

- [子 Agent 终态前总体仍是下界] → 保留已有 drain 与 incomplete 标记；本次不引入高频跨 actor 同步。
- [同一子 Agent 使用多个模型时可能出现重复 fold] → 继续依赖现有单次终态发送及 ACK 边界；每个 model row 同时只 fold 一次到该 Agent 与总体。
- [Agent 分项当前不可由 UI 检查] → 在 chat-state 和 shell 测试直接断言身份与算术，并在 wire 测试断言 Agent 分项未泄露。

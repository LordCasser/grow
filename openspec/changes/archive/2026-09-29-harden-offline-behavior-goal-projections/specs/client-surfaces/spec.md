## ADDED Requirements

### Requirement: Offline Behavior and Goal projections remain observational

Export 与 replay SHALL 展示已记录的 Behavior identity、Plan phase 与控制 terminal。模式选择、Goal state、foreground turn、Workflow Run SHALL 保持独立归属；展示 SHALL NOT 发起选择、确认、审批、自动续跑或终止工作。

#### Scenario: Goal changes during a foreground turn
- **WHEN** 一个 prompt 正文期间记录 Goal 设置、暂停或退出模式，随后旧 turn terminal 和新用户/自动续轮到达
- **THEN** 模式/Goal 更新保留原 prompt 正文归属，新 turn 按自己的 identity 展示，不从旧 turn cancelled 推导 Goal 停止，也不生成 continuation。

#### Scenario: Goal is cleared or replaced
- **WHEN** 空 ID 的 cleared 更新撤销当前 Goal，或者新 Goal 替换旧 Goal
- **THEN** 旧 Goal 不再作为当前展示，后续旧 ID 更新不将其复活；未 clear 的 paused Goal 可按已记录 active 更新重启。

#### Scenario: Goal stops and late usage arrives
- **WHEN** 已记录 paused、blocked、budget_limited 或 complete Goal，随后出现该状态的用量更新
- **THEN** 展示记录的停止状态与预算/用量，不把 TurnCompleted 或用量变化解释为 active；没有预算保持无预算。

#### Scenario: Plan selection needs confirmation or is rejected
- **WHEN** 历史 CurrentModeUpdate 保持原模式并携带 confirmation/rejection，或存在相应控制 terminal
- **THEN** 展示原模式及已有说明，不提前切到请求目标、不弹出可提交的审批。

#### Scenario: Workflow update is stale or Behavior has changed
- **WHEN** 更旧或重复 revision 在较新 Run 投影后到达，或用户已离开 Workflow Behavior
- **THEN** 旧投影不回退 Run，模式变化不自行结束 Run；cleared 不产生第二条活动 Run。

### Requirement: Offline Control snapshots use captured authority

来源 reader SHALL 校验捕获 Timeline 中的 Control snapshot，并在必要的只读末尾投影保留最新 Behavior/Goal 状态。该 snapshot SHALL 明确表示截点状态，未知中间事件顺序 SHALL 保持估计，不用当前机器时间伪造历史。

#### Scenario: Display cache omits the last Goal transition
- **WHEN** 捕获 Timeline 已提交退出/clear Goal，而 updates 中最后状态仍为 active
- **THEN** 结尾 snapshot 恢复已提交模式/Goal 截点，既有历史正文顺序不被猜测重排，来源文件不变。

#### Scenario: Control is invalid
- **WHEN** Control revision、architecture 或 Behavior/Goal ownership 不合法
- **THEN** 离线读取明确失败，不静默展示冲突控制状态。

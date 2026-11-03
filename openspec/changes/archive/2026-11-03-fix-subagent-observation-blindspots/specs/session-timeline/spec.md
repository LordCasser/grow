## ADDED Requirements

### Requirement: Public task observations share one call deadline
任务输出和 ACP 子任务查询 SHALL 从公共入口使用同一个绝对期限覆盖识别、队列、等待和结果读取。非阻塞查询总期限 SHALL 为五秒；任务输出显式等待 SHALL 使用 timeout_ms 与现有工具 cap 的较小值作为整次调用预算。各 ID 和来源读取 SHALL 不重开预算。等待取消 SHALL 保留完成通知的正确收据责任。

#### Scenario: A terminal mailbox or multiple child replies stall
- **WHEN** terminal 探测不回复，或批量 child 的查询不回复
- **THEN** 调用在共享期限结束时返回对应 observation_timed_out，保留全部 ID，不串行叠加五秒预算。

#### Scenario: Backend returns Running after a shorter segment
- **WHEN** backend 单次等待上限早于公共等待期限且 child 仍运行
- **THEN** 等待继续使用剩余期限等待真实状态，不声称其他任务完成，也不把配置时长当实测时长。

#### Scenario: Completion and observation uncertainty are aggregated
- **WHEN** 输出包括 completed、failed、cancelled、timed_out 或 output_unavailable
- **THEN** 单项判定和摘要将这些执行终态计入完成，列表仅在存在结果且全部项终态时为终态，空发现清单不代表执行终态；running 和 observation_* 不计入完成。

### Requirement: Authorized subagent observations agree across public entries
模型与 ACP 的 parent-owned 查询 SHALL 复用相同来源授权与 canonical 历史结果核对，在 coordinator 缺项或不可达时仍可于原期限内返回可信终态。prompt_status SHALL 以所需生命周期事实及引用验证状态，不因无关模型产物超过诊断读取额度而拒绝可信状态。所有入口 SHALL 保留 root/direct-parent、隐藏 Workflow 与精确来源链接约束。

#### Scenario: Historical child is queried through ACP after coordinator loss
- **WHEN** 合法来源查询可验证 Spawn/Seed/Ended/Result 的原 child，coordinator 缺项或通道关闭
- **THEN** ACP 与模型入口返回相同终态及谱系；未结束或无效证据仍为观察未知，越权和隐藏目标仍拒绝。

#### Scenario: Lifecycle evidence exceeds diagnostic body budget
- **WHEN** child 或 parent 存在大量无关采样产物，但 canonical 生命周期引用有效
- **THEN** prompt_status 可确认该生命周期，不加载无关正文；链接损坏仍报告无效证据。

### Requirement: Interrupted waits preserve identities without inventing execution state
用户或 parent intervention 只中断观察时 SHALL 为全部被查询身份保留结构化 observation_interrupted 结果，不据此推断 running 或 cancelled。已成功返回的正常工具结果和完成通知收据所有权 SHALL 保持原有优先与转移规则。

#### Scenario: A multi-task wait is interrupted
- **WHEN** 多项等待因 intervention 退出且没有获得目标状态
- **THEN** 按实际工具执行共用的身份解析保留全部 ID 为观察中断且不计入完成，后续完成通知仍可正确交付。

### Requirement: Active child inventories retain unknown progress
list_running SHALL 保持原有 active 和来源权限范围，缺少 progress 样本的项 SHALL 显式表达计数未知并保留身份，不遮蔽其他已知项，不伪造零计数。

#### Scenario: One active child has no progress sample
- **WHEN** 同一来源拥有正常样本和缺样本的 active child
- **THEN** 列表返回两项，正常项保留事实，缺样本项明确未知，隐藏或无关 child 不泄漏。

### Requirement: Progress sampling cancellation covers signal observation
采样器的现有 child cancellation token SHALL 覆盖 signals snapshot 等待，在 actor 不回复时仍能释放采样 owner；单项 UI 在途交付和本地样本更新 SHALL 保持原有边界。

#### Scenario: Signals actor retains but does not answer a snapshot request
- **WHEN** sampler 等待 signals 回复期间 runner 退出或其 child token 取消
- **THEN** sampler 退出并释放 progress owner，无需等待 signals actor 被销毁，不反向取消任务或排队额外交付。

### Requirement: Main agents can rediscover live child identities through task observation
主模型 SHALL 可通过现有任务输出入口按需枚举当前来源获准观察的 active child，即使压缩或先前文字遗漏其 ID，也不依赖旧 roster 或文件改动推断清理。发现 SHALL 沿用同一调用期限和来源权限；缺进度、正在收尾与真正执行终态 SHALL 保持区分。消息路由失败 SHALL 不被接口表达成已清理或已终态；直接父级消息授权 SHALL 保持。

#### Scenario: A still executing child is missing from the model's remembered task list
- **WHEN** 模型在支持 subagent 的任务输出入口以空 task_ids、非阻塞方式请求重新发现当前活跃任务
- **THEN** 返回当前 coordinator 持有且来源可见的 child 身份和状态，包括缺进度项；其他来源与隐藏 Workflow child 继续过滤，失败保持观察未知；无 subagent backend 的 terminal-only 入口仍要求非空身份，不枚举共享命令列表。

#### Scenario: A child cannot be addressed over a direct message route
- **WHEN** 消息返回无可用直接路由
- **THEN** 接口明确路由失败不证明目标已终态或已清理，调用方可按任务身份继续状态核对，不绕过直接父级权限。

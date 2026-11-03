## MODIFIED Requirements

### Requirement: Model subagent queries retain access to authorized durable results

模型子任务详情与等待查询在 coordinator 无该 child 注册时 SHALL 在原查询期限内只读核对当前来源的 durable Spawn、Ended 和适用的 child Result 链。lifecycle root 和 immediate parent SHALL 保留原有读取权限；无关来源、未知 ID 及隐藏的 Workflow child SHALL 保持授权拒绝且不泄漏存在性。历史查询 SHALL 使用有界只读 worker，不创建 writer、恢复执行或重新投递结果；有授权 Spawn 但无可信终态 SHALL 保持观察未知。

#### Scenario: Parent queries an original child after cold recovery
- **WHEN** 父会话恢复后 coordinator 没有原 child，但其 durable Spawn/Ended/Result 链可验证
- **THEN** 单项和多项详情返回原 completed、failed 或 cancelled 状态及可信统计；wait_subagent可立即返回适用最终结果，详情不读取答案artifact，不把完成缓存缺项解释为权限丢失。

#### Scenario: Unrelated or hidden source is queried
- **WHEN** 来源未获 root/direct-parent 授权，或目标是 Workflow 隐藏 child 或未知 ID
- **THEN** 查询继续拒绝，不读取或泄漏目标结果。

#### Scenario: Historical result cannot be proven
- **WHEN** 来源已授权但无 terminal、Result 链损坏或只读查询超时
- **THEN** 返回对应观察未知或无效证据，不从文件改动、注册表缺项或部分正文生成任务终态。

### Requirement: Task output distinguishes observation errors from confirmed execution

子任务详情和等待入口的单项和多项结果 SHALL 将不可达、超时、无效证据分别表达为 observation_unavailable、observation_timed_out、observation_invalid_evidence，不将这些观察失败报告为 running 或任何任务终态。观察失败 SHALL 保留任务身份和未确认结果；授权拒绝继续独立处理。

#### Scenario: Subagent status cannot be observed
- **WHEN** 单项或多项 get_subagent_status / wait_subagent 查询得到不可达、超时或无效证据
- **THEN** 结果返回对应观察失败状态和未知说明，既不确认运行，也不计入已完成数量。

### Requirement: Public task observations share one call deadline
命令输出、子任务枚举/详情/等待和 ACP 子任务查询 SHALL 从公共入口使用同一个绝对期限覆盖识别、队列、等待和结果读取。非阻塞查询总期限 SHALL 为五秒；命令输出与 wait_subagent 显式等待 SHALL 使用 timeout_ms 与现有工具 cap 的较小值作为整次调用预算。各 ID 和来源读取 SHALL 不重开预算。等待取消 SHALL 保留完成通知的正确收据责任。

#### Scenario: A terminal mailbox or multiple child replies stall
- **WHEN** 命令观察的terminal探测不回复，或subagent详情/等待的批量查询不回复
- **THEN** 调用在共享期限结束时返回对应 observation_timed_out，保留全部 ID，不串行叠加五秒预算。

#### Scenario: Backend returns Running after a shorter segment
- **WHEN** backend 单次等待上限早于公共等待期限且 child 仍运行
- **THEN** 等待继续使用剩余期限等待真实状态，不声称其他任务完成，也不把配置时长当实测时长。

#### Scenario: Completion and observation uncertainty are aggregated
- **WHEN** 输出包括 completed、failed、cancelled、timed_out 或 output_unavailable
- **THEN** 单项判定和摘要将这些执行终态计入完成，列表仅在存在结果且全部项终态时为终态，空发现清单不代表执行终态；running 和 observation_* 不计入完成。

### Requirement: Main agents can rediscover live child identities through task observation
主模型 SHALL 可通过专用list_subagent入口按需枚举当前来源获准观察的 未终态或观察未知的child，即使压缩或先前文字遗漏其 ID，也不依赖旧 roster 或文件改动推断清理。发现 SHALL 沿用同一调用期限和来源权限；缺进度、正在收尾与真正执行终态 SHALL 保持区分。消息路由失败 SHALL 不被接口表达成已清理或已终态；直接父级消息授权 SHALL 保持。

#### Scenario: A still executing child is missing from the model's remembered task list
- **WHEN** 模型通过list_subagent(scope=active)请求重新发现当前未终态任务
- **THEN** 返回当前来源可见的运行期与持久child身份及基本状态，包括缺进度、初始化、挂起、收尾与未确认项；其他来源与隐藏 Workflow child 继续过滤，失败保持观察未知；subagent入口不读取Terminal或枚举共享命令列表。

#### Scenario: A child cannot be addressed over a direct message route
- **WHEN** 消息返回无可用直接路由
- **THEN** 接口明确路由失败不证明目标已终态或已清理，调用方可按任务身份继续状态核对，不绕过直接父级权限。

## ADDED Requirements

### Requirement: Subagent tools separate enumeration detail and completion waiting
模型工具 SHALL 分离为list_subagent、get_subagent_status和wait_subagent。list_subagent SHALL 使用scope=active或all枚举当前来源授权的任务，仅返回subagent_id、类型、描述、基本状态、适用时间与错误信息，不含工具调用明细或最终答案。active SHALL 包括未终态或观察未知的任务；all SHALL 包括同一来源可见的持久历史。详情 SHALL 使用非空subagent_ids数组查询状态、计数和时间（不读取答案）；等待及适用最终结果交付 SHALL 仅通过wait_subagent调用，同样保留全部目标身份；详情与枚举不消费完成通知。command SHALL 通过get_terminal_command_output独立读取，不注册旧get_task_output/get_command_or_subagent_output混合入口，不保留空数组发现协议。

#### Scenario: List active and historical children without loading payloads
- **WHEN** 当前来源具有初始化、运行、挂起、收尾与历史终态child，并选择active或all
- **THEN** active返回未终态和未知身份，all还返回可验证历史终态；基本结果不含tool调用或最终输出，不依赖完成缓存保留或答案artifact存在。

#### Scenario: List across runtime loss or invalid history
- **WHEN** coordinator不可达但source有canonical Spawn/Ended/Result，或某个链接损坏
- **THEN** 可信历史终态保留，开放Spawn保持观察未知，损坏项明确无效证据；source历史不可读不能返回一份伪称all的runtime缓存清单。

#### Scenario: Query multiple child details with known and unknown metrics
- **WHEN** get_subagent_status接收一个或多个已授权subagent_ids
- **THEN** 每项返回真实状态、持续时间和可确认的调用次数等统计；未取得的字段明确未知，不伪造0，失败或取消的已有统计仍保留。

#### Scenario: Wait without mixing observation and execution outcomes
- **WHEN** wait_subagent接收目标ID组和等待预算
- **THEN** 它在共享期限内等待，分段Running不被当作结束，返回真实耗时/结束原因/各项目标状态；中断不取消任务或丢掉完成通知责任。

#### Scenario: Command and subagent capabilities remain distinct
- **WHEN** 工具preset、能力依赖、通知提示和等待UI生成新的工具集合
- **THEN** command使用get_terminal_command_output，subagent分别使用三个专用入口；无Terminal资源的subagent查询仍可运行，详情/列表不被误标阻塞等待。

#### Scenario: Metadata observation does not consume completion payloads
- **WHEN** list_subagent或get_subagent_status确认一个任务已终态但只交付基本信息或统计
- **THEN** 它们不消费该任务的完成通知；wait_subagent交付真实终态结果后才按原收据机制确认消费。

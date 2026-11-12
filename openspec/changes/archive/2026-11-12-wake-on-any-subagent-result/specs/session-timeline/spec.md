## MODIFIED Requirements

### Requirement: Subagent tools separate enumeration detail and completion waiting
模型工具 SHALL 分离为list_subagent、get_subagent_status、wait_subagent和get_subagent_result。list_subagent SHALL 默认使用scope=active，并通过显式scope=all枚举历史。两种范围均枚举当前来源授权的任务，仅返回subagent_id、类型、描述、基本状态、适用时间与错误信息，不含工具调用明细或最终答案。active SHALL 包括未终态或观察未知的任务；all SHALL 包括同一来源可见的持久历史。详情 SHALL 使用非空subagent_ids数组查询状态、计数和时间（不读取答案）；阻塞等待及适用最终结果交付 SHALL 通过wait_subagent调用，同样保留全部目标身份。get_subagent_result SHALL 按单一subagent_id读取当前状态及可验证的最终答案；可重复查询且不等待子任务进入终态。详情、枚举和单次结果读取不消费完成通知。command SHALL 通过get_command_output独立读取，不注册旧get_task_output/get_command_or_subagent_output混合入口，不保留空数组发现协议。

#### Scenario: List active and historical children without loading payloads
- **WHEN** 当前来源具有初始化、运行、挂起、收尾与历史终态child，并选择active或all
- **THEN** 省略scope或选择active时返回未终态和未知身份，已有canonical Ended的历史项不打开child日志验证；all还返回可验证历史终态及其状态；基本结果不含tool调用或最终输出，不依赖完成缓存保留或答案artifact存在。

#### Scenario: List across runtime loss or invalid history
- **WHEN** coordinator不可达但source有canonical Spawn/Ended/Result，或某个链接损坏
- **THEN** all保留可信历史终态，开放Spawn保持观察未知，损坏项明确无效证据；source历史不可读不能返回一份伪称all的runtime缓存清单。

#### Scenario: Query multiple child details with known and unknown metrics
- **WHEN** get_subagent_status接收一个或多个已授权subagent_ids
- **THEN** 每项返回真实状态、持续时间和可确认的调用次数等统计；未取得的字段明确未知，不伪造0，失败或取消的已有统计仍保留。

#### Scenario: Wait without mixing observation and execution outcomes
- **WHEN** wait_subagent接收目标ID组和等待预算
- **THEN** 它在共享期限内等待，任一目标确认终态即以 any_finished 返回该目标 ID、真实状态和可用结果，不等待其他目标；分段 Running、挂起与收尾不算终态。返回真实耗时及全部目标身份，保留最近实际观察，提前结束时未观察项明确 observation_interrupted；中断观测不取消任务或丢掉未交付终态的完成通知责任。

#### Scenario: Command and subagent capabilities remain distinct
- **WHEN** 工具preset、能力依赖、通知提示和等待UI生成新的工具集合
- **THEN** command使用get_command_output，subagent分别使用四个专用入口；无Terminal资源的subagent查询仍可运行，详情/列表不被误标阻塞等待。

#### Scenario: Metadata observation does not consume completion payloads
- **WHEN** list_subagent或get_subagent_status确认一个任务已终态但只交付基本信息或统计
- **THEN** 它们不消费该任务的完成通知；wait_subagent交付真实终态结果后才按原收据机制确认消费。

#### Scenario: Command output has one concise registered name
- **WHEN** 当前工具集注册、发现或调用后台命令输出工具
- **THEN** 仅使用get_command_output；旧get_terminal_command_output不作为别名提供，参数、权限与command-only行为保持。

#### Scenario: Read one child result again
- **WHEN** 模型使用 get_subagent_result 查询已授权的单一 subagent ID
- **THEN** 可重复取得该 ID 的可验证终态和最终答案。未终态或证据不可用时返回对应状态，且不执行、恢复、取消子任务或消费完成通知。

#### Scenario: One child finishes while another remains blocked
- **WHEN** 同一 wait_subagent 等待 A 和 B，A 已完成而 B 仍未返回观察
- **THEN** 本次等待立即返回 any_finished 和 A 的 ID、终态及答案，B 保持原执行且未观察状态明确未知，不等 B 或整次预算。

#### Scenario: Failure and cancellation wake the parent
- **WHEN** 任一目标确认 failed、cancelled 或 output_unavailable，其余目标未完成
- **THEN** 等待以 any_finished 返回对应目标的真实终态及原因，不将它解释为成功结果；没有真实终态的单项观察失败不触发 any_finished。

#### Scenario: Unfinished observations and receipts survive early return
- **WHEN** A 完成前已观察到 B 的非终态，C 尚无观察
- **THEN** 返回 B 的最近实际状态与 C 的 observation_interrupted，只确认实际交付终态的完成通知，其余目标继续执行并保留完成通知责任。

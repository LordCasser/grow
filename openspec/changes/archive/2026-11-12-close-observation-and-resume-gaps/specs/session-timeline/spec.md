## MODIFIED Requirements

### Requirement: Authorized subagent observations agree across public entries
模型与 ACP 的 parent-owned 查询 SHALL 复用相同来源授权与 canonical 历史结果核对，在 coordinator 缺项或不可达时仍可于原期限内返回可信终态。prompt_status SHALL 以所需生命周期事实及引用验证状态，不因无关模型产物超过诊断读取额度而拒绝可信状态。所有入口 SHALL 保留 root/direct-parent、隐藏 Workflow 与精确来源链接约束。

#### Scenario: Historical child is queried through ACP after coordinator loss
- **WHEN** 合法来源查询可验证 Spawn/Seed/Ended/Result 的原 child，coordinator 缺项或通道关闭
- **THEN** ACP 与模型入口返回相同终态及谱系；未结束或无效证据仍为观察未知，越权和隐藏目标仍拒绝。

#### Scenario: Lifecycle evidence exceeds diagnostic body budget
- **WHEN** child 或 parent 存在大量无关采样产物，但 canonical 生命周期引用有效
- **THEN** prompt_status 可确认该生命周期，不加载无关正文；链接损坏仍报告无效证据。

#### Scenario: Resident source contains unrelated conversation history
- **WHEN** 枚举或查询使用 resident 来源且存在大量无关历史
- **THEN** 由 owner 返回已接纳的窄生命周期事实，不扫描或复制无关 Timeline 事件，并保持 Seed/Spawn 与结果链接验证。

#### Scenario: Coordinator accepts the query but never replies
- **WHEN** 已授权查询的 runtime 端保持沉默，但来源历史可以确认终态
- **THEN** 在原绝对期限内返回经验证的终态，历史读取不等待 runtime 超时才启动。缺项或观察错误不得盖过有效 runtime 响应。

### Requirement: Subagent tools separate enumeration detail and completion waiting
模型工具 SHALL 分离为list_subagent、get_subagent_status、wait_subagent和get_subagent_result。list_subagent SHALL 默认使用scope=active，并通过显式scope=all枚举历史。两种范围均枚举当前来源授权的任务，仅返回subagent_id、类型、描述、基本状态、适用时间与错误信息，不含工具调用明细或最终答案。active SHALL 包括未终态或观察未知的任务；all SHALL 包括同一来源可见的持久历史。详情 SHALL 使用非空subagent_ids数组查询状态、计数和时间（不读取答案）；阻塞等待及适用最终结果交付 SHALL 通过wait_subagent调用，同样保留全部目标身份。get_subagent_result SHALL 按单一subagent_id读取当前状态及可验证的最终答案；可重复查询且不等待子任务进入终态。详情和枚举不消费完成通知。主动读取成功交付可验证最终答案时，仅确认同一子任务的完成通知，原答案仍可重复读取。command SHALL 通过get_command_output独立读取，不注册旧get_task_output/get_command_or_subagent_output混合入口，不保留空数组发现协议。

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
- **THEN** 可重复取得该 ID 的可验证终态和最终答案。未终态或证据不可用时返回对应状态，且不执行、恢复、取消子任务。

#### Scenario: One child finishes while another remains blocked
- **WHEN** 同一 wait_subagent 等待 A 和 B，A 已完成而 B 仍未返回观察
- **THEN** 本次等待立即返回 any_finished 和 A 的 ID、终态及答案，B 保持原执行且未观察状态明确未知，不等 B 或整次预算。

#### Scenario: Failure and cancellation wake the parent
- **WHEN** 任一目标确认 failed、cancelled 或 output_unavailable，其余目标未完成
- **THEN** 等待以 any_finished 返回对应目标的真实终态及原因，不将它解释为成功结果；没有真实终态的单项观察失败不触发 any_finished。

#### Scenario: Unfinished observations and receipts survive early return
- **WHEN** A 完成前已观察到 B 的非终态，C 尚无观察
- **THEN** 返回 B 的最近实际状态与 C 的 observation_interrupted，只确认实际交付终态的完成通知，其余目标继续执行并保留完成通知责任。

#### Scenario: A result read reaches the model before its completion reminder
- **WHEN** get_subagent_result 交付含答案的 Completed，且相应完成通知仍待交付或尚未接纳
- **THEN** 同一 Timeline 工具结果记录确认交付身份，已待发或迟到的回执均不会重复投递，冷恢复沿相同事实重建。再次读取仍返回原答案。答案不可用、非终态或观察失败不确认回执。

### Requirement: Suspension acknowledges a durable tree boundary

挂起 SHALL 在停止后继执行准入后核对参与节点及其持久续跑位置，只在全部必要事实确认后报告 Suspended。重复请求 SHALL 复用同一次挂起身份。ACK 丢失、写入失败、节点不可达或等待期限到达 SHALL 报告未确认，保留恢复依据和仍在途的写入所有权，不推断未写入或虚构成功。部分挂起 SHALL 不自动重启已经停住的节点。

#### Scenario: All participating nodes acknowledge
- **WHEN** root 和所有参与 child 的续跑位置、结算与依赖引用均已持久确认
- **THEN** root 可确认整树 Suspended，资源关闭沿原 writer drain 规则完成，随后退出。

#### Scenario: Checkpoint requires command settlement evidence
- **WHEN** 节点准备提交 checkpoint，且挂起 operation 尚未记录命令结算
- **THEN** Timeline 拒绝 checkpoint；节点先为同一 operation 持久记录一次 CommandsSettled（没有活跃命令时也记录空集合），相同内容的 ACK 重试幂等，冲突内容或不同 operation 被拒绝。

#### Scenario: Command runtime state was retired after settlement
- **WHEN** CommandsSettled 已持久确认，命令 runner 已退役或其完成缓存已淘汰，但 checkpoint 写入或 ACK 需要重试
- **THEN** 从同一 operation 的持久结算复用原 call/command 身份、完成或中断证据、后台属性与有界输出；不再次终止命令，不依赖已退役的内存缓存，不将已有证据改为空集合或 Unknown。

#### Scenario: Background command settlement reaches a durable checkpoint

- **WHEN** 挂起已持久记录一个后台命令的 CommandsSettled，且该命令的完成通知尚未接纳
- **THEN** 挂起 SHALL 以 Timeline 接受的终态通知身份持久接纳该结果，再提交节点 checkpoint；相同 operation 的重试 SHALL 复用命令结算和通知，不重新运行命令或写入第二个逻辑通知。

#### Scenario: Root acknowledgment is lost
- **WHEN** 所有节点已保存，但 root 挂起提交的 ACK 丢失
- **THEN** 重试或重启核对同一挂起事实，不追加另一份逻辑任务，不重新执行已保存的工作。

#### Scenario: Process exits during partial suspension
- **WHEN** 挂起意图已提交但只有部分节点保存后进程退出
- **THEN** 恢复先核对原节点与 writer，补齐可验证的挂起事实；无法证明续跑位置的节点保持恢复受阻，不将整树当作完整 checkpoint 自动启动。

#### Scenario: Checkpoint binds the exact runtime source
- **WHEN** SessionActor 持久化运行时 checkpoint
- **THEN** checkpoint 包含工作区、Agent 定义及其跳过序列化的能力源、selector/plugin、role prompt 与模型路线的 64 位十六进制 source hash；原始配置、凭据与 prompt 正文不进入 checkpoint，source hash 格式无效时 Timeline 拒绝写入。

目标入口：`crates/codegen/shell/src/session/timeline_persistence.rs`、`crates/codegen/shell/src/session/actor/spawn.rs`；复用 `session-timeline` 的完整 writer drain、exact ACK 和收尾核对契约。

#### Scenario: Settlement preserves command execution timing
- **WHEN** 挂起结算获得命令的执行快照
- **THEN** CommandsSettled 持久化实际执行起止时间，包括转后台前的执行时间；已有结算重试复用同一事实。

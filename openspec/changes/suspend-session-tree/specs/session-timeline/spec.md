## ADDED Requirements

### Requirement: Session tree suspension preserves logical work

会话 SHALL 将主动挂起记录为独立于完成、取消和失败的生命周期，覆盖指定 root 拥有的全部未结束递归 child、前后台工作和等待关联。挂起 SHALL 保留逻辑 session/task/prompt 身份、已提交进度、模型路线、工作区与结果交付责任；不得仅因运行实例退出生成 child cancelled/completed 结果。真实完成或已明确取消的任务 SHALL 保留原终态。

#### Scenario: Suspend a root with mixed child states
- **WHEN** root 等待两个 child，一个已经完成，另一个仍在执行且拥有更深层 child
- **THEN** 完成结果保持原值，未结束子树进入挂起，原父子等待关联保留；不会把挂起转换成取消或重新委派。

#### Scenario: Spawn competes with suspension
- **WHEN** 挂起关闭准入时另一个 child spawn 已进入提交或 ACK 不明
- **THEN** 挂起依据原身份核对该 spawn，纳入所有已准入 child；未准入者不启动，未知者不会被忽略后声称整树挂起成功。

#### Scenario: Child finishes during suspension
- **WHEN** child 的真实结果与挂起请求竞争
- **THEN** 已验证完成结果与待交付责任优先保留，不覆盖为 Suspended，也不重复结算结果和用量。

目标入口：`crates/codegen/shell/src/session/actor/run_loop.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`、`crates/codegen/chat-state/src/timeline.rs`。

### Requirement: Suspension acknowledges a durable tree boundary

挂起 SHALL 在停止后继执行准入后核对参与节点及其持久续跑位置，只在全部必要事实确认后报告 Suspended。重复请求 SHALL 复用同一次挂起身份。ACK 丢失、写入失败、节点不可达或等待期限到达 SHALL 报告未确认，保留恢复依据和仍在途的写入所有权，不推断未写入或虚构成功。部分挂起 SHALL 不自动重启已经停住的节点。

#### Scenario: All participating nodes acknowledge
- **WHEN** root 和所有参与 child 的续跑位置、结算与依赖引用均已持久确认
- **THEN** root 可确认整树 Suspended，资源关闭沿原 writer drain 规则完成，随后退出。

#### Scenario: Root acknowledgment is lost
- **WHEN** 所有节点已保存，但 root 挂起提交的 ACK 丢失
- **THEN** 重试或重启核对同一挂起事实，不追加另一份逻辑任务，不重新执行已保存的工作。

#### Scenario: Process exits during partial suspension
- **WHEN** 挂起意图已提交但只有部分节点保存后进程退出
- **THEN** 恢复先核对原节点与 writer，补齐可验证的挂起事实；无法证明续跑位置的节点保持恢复受阻，不将整树当作完整 checkpoint 自动启动。

目标入口：`crates/codegen/shell/src/session/timeline_persistence.rs`、`crates/codegen/shell/src/session/actor/spawn.rs`；复用 `session-timeline` 的完整 writer drain、exact ACK 和收尾核对契约。

### Requirement: Suspended trees resume the original identities automatically

用户通过执行型 resume 打开已挂起 root 时，系统 SHALL 先验证并重建原树的全部未结束节点、运行配置、工作区、等待与交付关联，再允许可运行节点继续。不得通过新 child identity、`resume_from` 派生任务或重新接纳原用户 prompt 代替恢复。重复或并发 resume SHALL 不启动重叠 writer、重复 child 或重复结果交付。只读查询、export、replay 和历史 child 查看 SHALL 不触发执行。

#### Scenario: Resume the suspended tree
- **WHEN** 用户恢复已确认挂起的 root，且全部必要来源与运行环境有效
- **THEN** 未结束 child 自动以原逻辑身份继续，完成 child 不再执行，父 Agent 按原依赖继续等待或消费原结果，无需模型重新规划或用户逐个恢复。

#### Scenario: A required node cannot be restored
- **WHEN** child 来源、运行路线、工作区或恢复 artifact 缺失、不匹配，或旧 writer 尚未退出
- **THEN** 整树保持执行准入关闭并指出受阻节点与原因；不静默换模型、创建空白 child 或先启动部分树。

#### Scenario: Resume is repeated after work has begun
- **WHEN** 同一挂起状态的 resume 请求重复、ACK 丢失或两个客户端并发恢复
- **THEN** 复用已接纳的恢复实例或返回其状态，不再次创建执行、消费输入或发出完成通知。

#### Scenario: A resumed runtime crashes without another suspension
- **WHEN** 挂起恢复资格已消费，节点已重新运行，随后无新的挂起事实就异常退出
- **THEN** 根据最新执行证据走既有异常恢复，不回退旧 checkpoint 或重发已派发工作；旧挂起记录不授予永久自动重启资格。

目标入口：`crates/codegen/shell/src/agent/mvp_agent/acp_agent.rs::load_session`、`crates/codegen/shell/src/agent/subagent/handle_request.rs`、`crates/codegen/tools/src/implementations/grow_build/task/coordinator.rs`。

### Requirement: Suspended operations retain interruption evidence

快速挂起 SHALL 请求中止活跃工具和后台命令，不等待其业务自然完成。操作结果 SHALL 区分已确认完成、确认终止但未完成的 Interrupted、无法确定执行或副作用的 OutcomeUnknown，以及确认尚未派发。取消信号、超时或断线本身 SHALL NOT 证明操作已终止或未产生副作用。命令中断 SHALL 不终结其所属 Agent 任务。

#### Scenario: Local command termination is confirmed
- **WHEN** 挂起中确认原命令及受控进程组停止，但命令没有正常完成
- **THEN** 保存原调用身份、可得输出、终止证据及 Interrupted 结果；所属 child 挂起并可继续，不伪造业务成功。

#### Scenario: Remote operation has an unknown outcome
- **WHEN** MCP、远程命令或脱离控制的进程无法确认停止或副作用结果
- **THEN** 保存 OutcomeUnknown 及可核对的原操作信息，明确外部工作可能仍在运行；挂起仅确认 Grow 自身停止推进，不声称外部世界已冻结。

#### Scenario: Resume encounters an interrupted or unknown call
- **WHEN** 恢复后 Agent 继续处理被中断或结果未知的操作
- **THEN** 获得对应事实并检查现场；恢复机制不自动重发原调用，也不以“继续”提示指示盲重试。确认中断后的新调用遵循正常授权；未知副作用先核对或取得明确处置决定。

#### Scenario: A batch contains completed and undispatched calls
- **WHEN** 同一批工具已有完成结果，另一些调用可证明尚未派发
- **THEN** 复用已保存结果，保留未派发调用及原身份，经当前权限检查后继续；不重新执行整个批次。

目标入口：`crates/codegen/shell/src/session/actor/tool/{mod,dispatch,result}.rs`、命令/task runner 与现有 MCP transport 取消边界。

### Requirement: Suspended waits and deliveries remain attributable

父子等待、人工问题、待交付结果与已挂起的调用 SHALL 保留稳定关联。恢复 SHALL 重建实际等待，不把临时 Future、channel 或 waiter 消失解释为任务完成。每个 tool call SHALL 至多提交一份 tool_result；已提交中断或未知结果的调用，其迟到事实 SHALL 经既有通知或观测路径交付。

#### Scenario: Parent is waiting for a suspended child
- **WHEN** 父调用正在等待 child，而 child 达到挂起位置
- **THEN** 父调用保存对原 child 的等待，不要求 child 完成才能挂起；恢复后重新绑定该 child，不收到虚构取消结果。

#### Scenario: Result was committed before delivery
- **WHEN** child result 已确认而父回执尚未完成时发生挂起
- **THEN** 恢复只补原身份的交付，保留一次性的用量累计，不重新运行 child。

#### Scenario: A late result follows an unknown tool result
- **WHEN** 原 call 已提交 OutcomeUnknown tool_result，后续取得可验证最终事实
- **THEN** 保留原未知事实并追加可归属的新事实，不为同一 call 写入第二份 tool_result。

目标入口：`crates/codegen/shell/src/session/actor/completion_delivery.rs`、`notification_drain.rs`、`crates/codegen/shell/src/agent/subagent/mod.rs`。

### Requirement: Suspended recovery references protect their resources

未恢复或恢复受阻的挂起事实 SHALL 保护必要的 child 存储、worktree、脚本、prompt/result artifact 和交付引用；运行实例退出或完成缓存淘汰 SHALL NOT 授权清理这些依赖。fork、导入副本或 rewind SHALL NOT 复制原树的执行所有权，只有原树的合法 writer 可消费其挂起恢复资格。

#### Scenario: Cleanup sees suspended work
- **WHEN** 自动 TTL、任务缓存淘汰或工作区清理遇到仍被挂起树引用的资源
- **THEN** 保留该资源或报告清理受阻，不破坏原任务的恢复依据。

#### Scenario: History is copied or rewound
- **WHEN** 用户 fork、导入或回退包含挂起记录的历史
- **THEN** 复制的历史不启动原 child；会破坏当前挂起引用的 destructive 操作须沿现有删除/所有权核对拒绝或先明确终止原工作。

目标入口：`crates/codegen/shell/src/session/storage/`、子 Agent worktree 清理及 `session-timeline` 的恢复引用删除护栏。

## MODIFIED Requirements

### Requirement: Turn terminals identify their actual authority

新 Turn terminal SHALL 明确区分 provider 响应驱动、宿主控制/错误、用户取消和中断恢复。provider 驱动终态 SHALL 关联相同 Turn 的已完成 request，其原始 provider terminal 由 request/attempt 证据保存。宿主 stop_reason/completion_kind SHALL 仅是本地分类。历史未记录来源的终态 SHALL 保持未知，不据旧字符串猜测或回写。存在未消费的有效挂起意图或 checkpoint 时 SHALL 先核对挂起边界；有效挂起保留原逻辑 Turn，不生成 Recovery terminal。物理 request/工具仍按事实记录中断和结算，不能据此终结所属逻辑 Turn；挂起证据不足时保持恢复受阻。

#### Scenario: Natural response closes the turn
- **WHEN** 完整正常 provider 响应使普通 Turn 收尾
- **THEN** Turn terminal 标识 Provider 来源并关联其 request，可追溯到对应 attempt 的原生终止。

#### Scenario: Host stops after a provider response
- **WHEN** provider 已结束，但宿主因预算、控制、持久化错误或用户取消收尾
- **THEN** 保留原始响应证据，同时追加宿主或用户来源的 Turn terminal，不覆盖原始原因。

#### Scenario: Process recovery closes an open turn
- **WHEN** 重启时发现未闭合生命周期，且没有尚可消费的挂起续跑依据
- **THEN** 追加 Recovery 来源的 interrupted 终态；不伪造 provider 事件或正常成功。

#### Scenario: Suspended logical turn continues under its original identity
- **WHEN** root 或 child 的原 Turn 具有可验证的挂起续跑位置，且恢复获得合法执行所有权
- **THEN** 保留该 Turn 和既有已提交前缀，在原位置继续，不重复 TurnStarted 或接纳原 prompt；只有后续真实完成、取消或失败才按实际来源生成一次 Turn terminal。

#### Scenario: Historical source is absent
- **WHEN** 读取没有记录来源的旧终态
- **THEN** 来源保持 Unknown，不把 end_turn 推断为 provider 返回。

### Requirement: Recovered child outcomes preserve evidence and availability

已经验证的 child terminal outcome SHALL 保持不变，正文可用性与交付失败分别报告。只有证明原执行实例已结束、合法 writer 已取得所有权且无有效终态或有效挂起续跑依据时，恢复才可在结算未闭合 scope 后记录 Recovery 来源的 interrupted 结果。存在未消费的挂起意图或节点 checkpoint 时 SHALL 先按挂起协议核对；有效挂起不生成 child terminal，部分挂起证据不足时保持恢复受阻。未知用量 SHALL 保持 incomplete，未知时间不得以整个离线间隔冒充执行耗时。

#### Scenario: Completed child loses its output artifact
- **WHEN** child terminal 及父子身份可验证，但结果正文文件缺失或校验失败
- **THEN** 已确认 outcome 仍可只读展示，并明确正文不可用；不得改写成新的失败 outcome 或拼凑正文交付。

#### Scenario: Inspection failed while child may still run
- **WHEN** backend 不可达或超时，无法确认旧实例已退出
- **THEN** 保持收尾待确认，不生成 interrupted/cancelled 结果，也不抢占 writer。

#### Scenario: Cold recovery finds an interrupted child
- **WHEN** 新 writer 验证原实例已结束，没有有效 child terminal 或尚可消费的挂起续跑依据，且还有未闭合执行或结算
- **THEN** 先按既有结算规则恢复这些 scope，再记录明确的恢复中断来源及用量下界；不伪造用户取消或零消费。

#### Scenario: Cold recovery finds a suspended child
- **WHEN** 新 writer 找到与 root 挂起操作匹配的 child 续跑位置
- **THEN** 分流到挂起核对与同身份恢复，不由孤立 child 修复生成 interrupted terminal。

证据与实施入口：`crates/codegen/shell/src/agent/subagent/mod.rs::result_from_inspection/finish_from_durable_facts/reconcile_orphaned_subagents_with_backend`、`crates/codegen/chat-state/src/timeline.rs::SubagentResultEvent`。

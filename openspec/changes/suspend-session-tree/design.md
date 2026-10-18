## Context

动机及本次仅交付提案的范围见 `proposal.md`。以下是 `0a0dd07c` 的静态核对结果，不是已实现的挂起能力。

| 当前入口 | 当前行为 | 本 change 的调整点 |
| --- | --- | --- |
| `pager/src/app/root/mod.rs`、`root/dispatch/turn.rs` | 运行时 Ctrl+C 可能清草稿、CancelTurn 或进入 Goal 中断选择；退出还有确认分支 | 全局退出进入 Suspend，不先改变任务业务状态 |
| `pager/src/app/signal_handler.rs`、`acp/spawn.rs` | 首次 OS 信号走 quit，第二次强退；退出 guard 关闭 worker | 受控 quit 先挂起，guard 识别已交接状态；保留真实紧急强退 |
| `shell/src/session/actor/run_loop.rs::begin_graceful_shutdown` | 拒绝排队输入，取消前台，关闭 sampler，取消并 drain child | 独立挂起入口及资源退役路径，不能以调用现有 Cancel 模拟 |
| `shell/src/session/actor/spawn.rs` | 冷加载先 `recover_interrupted_durably`，再恢复输入 | 在普通中断修复前识别并核对挂起事实 |
| `shell/src/agent/mvp_agent/acp_agent.rs::load_session` | 冷 root 加载后核对孤立 child | 在可执行发布前重建挂起树，绕开错误 terminalization |
| `tools/src/implementations/grow_build/task/coordinator.rs` | pending/active/completed/finalizations 在内存；取消和 Drop 会终结 child | 增加挂起结果及原身份 adoption/rebind，不落入 finish/cancel |
| `shell/src/agent/subagent/handle_request.rs::run_shell_child` | `resume_from` 校验已结束来源，创建派生 child，接纳新 prompt | 增加原 child 的冷加载/续跑路径，保持旧 resume_from 的独立语义 |
| `shell/src/session/workflow/`、`workflow/src/journal.rs` | Run 源被冻结，journal 校验调用与未确认 child | 保留该机制，增加会话级挂起的执行轮次边界 |

表中代码路径均相对 `crates/codegen/`。Atlas 的局部索引只用于定位；行号和实际行为以当前文件为准。

现有相冲突契约已在 delta 中完整修改：client-surfaces 的 Ctrl+C 队列场景、退出 writer 和终端 teardown，以及 session-timeline 的未闭合 Turn 和孤立 child 中断恢复。合法 cancelled child 的显式 `resume_from` 继续存在；它不是本次自动恢复的入口。

## Goals / Non-Goals

**Goals**：快速停止 Grow 继续工作；保留原逻辑任务树；允许当前命令中断；下次显式执行型 resume 自动续接；每次副作用与交付有可验证来源。

**Non-Goals**：保存 Rust Future/线程栈、OS 进程内存或 provider 生成中的远端状态；任意工具安全重放；后台 daemon；跨设备执行；通用调度器/数据库替换；旧 schema 兼容层；未提交 composer 草稿的全新跨进程保存功能。

## Decisions

### 1. Separate logical task state from runtime suspension

树运行生命周期为 Running → Suspending → Suspended → Restoring → Running；失败是挂起/恢复操作受阻，不能直接写成任务 Failed。真实 Completed/Cancelled/Failed 仍由原 owner 决定，暂停期间先发生的合法终态不可覆盖。

保留 root/child session ID、task ID、逻辑 prompt/turn 及委派来源；恢复可以创建新的运行实例与 writer incarnation，但不创建新的逻辑 child。挂起的 child 不写 `SubagentResult(cancelled)` 或 parent `Ended(cancelled)`。coordinator 的 runner 返回类型必须能表达“运行实例已交出挂起责任”，不能只返回现有 terminal result 再偷偷改 UI 标签。

会话级 admission gate 与 Goal/Workflow 的业务状态相交。Goal Active 在 Suspended 时没有执行资格；原本 Paused/Blocked 的 Goal 或 await_user 的 Run 不会因 session resume 自动获得新业务授权。

### 2. Timeline owns the continuation, coordinators rebuild from it

在现有 Timeline 增加 typed 挂起/恢复事实，具体 Rust 名称随实现保持一致，不另建可变快照日志。树级操作身份复用持久事件身份；节点 checkpoint 引用该操作、节点身份与已提交 frontier。checkpoint 保存控制引用和恢复必需的小型状态，正文通过现有不可变 artifact 引用；不得复制整段历史或持久化 credentials、live permit、channel、future。

最小续跑内容：原 logical turn/step 归属、已接纳 response/call ID、待执行批次位置、原 child/operation 等待、人工问题身份、待交付回执，以及必要配置/工作区引用。源内容继续由已记录的 Timeline、Control、Workflow seed/journal 和 artifact 提供。

| 保存位置 | 恢复动作 |
| --- | --- |
| 原步骤尚无 accepted response | 保存前次中断证据；通过原采样安全、结算和剩余额度检查后，从原已提交上下文发起新 attempt |
| accepted response 含未派发调用 | 补齐既有 response 展示 gate，然后重新检查权限并执行剩余调用 |
| 工具结果已确认 | 复用结果，继续下一模型步骤 |
| AwaitChild | 绑定原 child，不向原调用返回虚构取消结果 |
| AwaitUser / AwaitPermission | 重建原问题对应的真实等待，保留待决状态 |
| child result 待交付 | 只推进既有父引用/回执，不重新执行 child |

挂起的逻辑 turn 不由通用 `recover_interrupted` 结束；物理 request/工具的中断事实仍如实闭合。恢复沿已有 logical turn 的续跑位置进入，不重发用户 prompt、不重复 TurnStarted。validator/fold 必须允许这一显式边界，并继续拒绝无挂起证据的悬空执行。

### 3. Quiesce the entire tree with one bounded request

root 持有一次挂起操作，现有 coordinator 向已准入后代传播。先关闭 root 与各 child 的后继准入（provider、tool、spawn、Goal continuation、Workflow host、后台 producer），再取消当前有限执行；不能只停止 UI 而让内部 timer 或 completion callback 再开工作。

spawn 与挂起共享现有准入串行化边界。对已准入和 ACK 不明的 spawn，先核对原事实确定成员，不用内存 active 列表或目录扫描代替权威；关联更深层后代时使用真实委派/ownership 链，不将 fork/resume 来源当后代。

节点提交 checkpoint 后，root 提交引用全部参与节点 frontier 的确认。跨节点用可验证引用和逐项核对，不宣称多文件原子事务。已完成节点以 canonical result 满足边界；未完成节点以 checkpoint 满足。父 AwaitChild 本身可挂起，不等 child 完成业务。

取消、节点保存、root 确认及 runtime drain 共用默认五秒绝对期限；重复按键和内部重试不重置。超时停止派发新的恢复写入，界面显示未完成，可显式重试或强退。超时不是取消仍在途的 I/O；writer lease 在其真实结束前继续有效。不能同时承诺遇到阻塞存储时五秒必然退出且完整落盘。

已持久 Suspended 但 writer 仍未退出时，界面区分“已保存，退出尚未完成”。再次加载仍等待完整 writer drain。只有 root 及必要节点确认、原 runtime 完全退役才报告成功退出；panic/SIGKILL 等没有完整协议的退出仍属于异常路径。

### 4. Interrupt operations without cancelling the Agent

本 change 采用用户明确选择的快速模式，不等待构建、测试或远程工具自然完成。向受控命令进程组请求终止，复用现有运行器的信号/回收能力；只在确认停止后记录 Interrupted。远程取消、无法回收的子进程或响应丢失保留 OutcomeUnknown，包含可得 operation ID、原参数/目标引用、输出和可核对证据。

命令的“停止确认”不证明它从未产生副作用；Interrupted 结果也须说明可能已有部分修改。确认完成的原结果即使在取消竞争中到达也保留，不改写为失败。

恢复给 Agent 原上下文及明确的中断/未知结果。runtime 不重发旧命令；Agent 可按现有权限做新的现场检查。需要重复执行时，已确认中断的操作按当前实际状态决策，未知副作用先核对或由用户明确处置，不能仅换 call ID 绕过未知事实。这里不建立任意 shell 文本的语义去重器或通用 reconcile 插件。

保存中断结果时维持一调用一份 tool_result。之后取得原操作最终事实使用有原身份的 observation/通知交付，不覆盖旧证据。AwaitChild 不属于普通外部命令中断：其等待引用保留，不生成 child 取消值。后台命令也必须参与退出盘点，不能因为前台 turn 已结束而漏掉。

活跃模型 attempt 立即请求取消，但必须结算可得 evidence/usage；partial candidate 不成为 canonical response。恢复只对尚未 accepted 的步骤检查重新采样资格，继续遵守 typed attempt 事实、输出可撤销性、provider 托管工具副作用安全、结算、owner/预算和恢复额度等既有条件。没有本地工具执行不等于没有远端副作用；无法证明安全时保留未知事实并阻止自动重采样。这不要求远端 provider 恢复原 token 流，也不重跑已完成工具。

保存并沿用同一逻辑采样已消耗的 attempt 数与绝对期限，不因挂起、新运行实例或重提交重新计数。绝对期限可能在离线期间到期，此时恢复报告该步骤的额度/期限失败，不能把 session 自动恢复理解为绕过所有步骤失败。Goal 的执行时长冻结与采样已有绝对期限是不同约束。

### 5. Restore the graph before releasing any execution

执行型 root resume 取得原树恢复所有权后，先读取并验证 root 操作与所有必要节点，确认旧 writer 已退出，再建立 coordinator 的 pending/active/adopted 记录与 Shell `active_child_sessions` 控制路由。普通 child 历史查看不能走该入口；从 child 执行入口遇到挂起树时应解析到所属 root，并按同一次树恢复处理，不能只启动一个脱离父等待的 child。

恢复装载期间全树 admission 保持关闭，不运行新的 SessionStart/SessionEnd Hook、不消费队列、不启动 Goal 自动续轮。旧实例普通 Drop 的 `cancel_all_children` 不能再接管已确认挂起的节点；相应资源交接必须显式完成。

全部必要节点已可重建才提交恢复接纳事实，随后开放可运行节点。重复 resume 或 ACK 丢失按原挂起/恢复身份核对。挂起恢复资格在接纳恢复时被单次消费，并记录节点进入新运行实例的进度；重启先核对该进度，能证明尚未开放执行的节点可以继续原恢复交接。

已经重新运行的节点若再次异常退出而没有新的挂起事实，沿既有异常中断恢复处理，不能退回旧 checkpoint、重复派发，也不能承诺旧挂起记录使以后所有崩溃都能自动续跑。已消费 checkpoint 仍保留审计价值，但不再授权新执行；异常路径按当前真实证据保留中断或未知状态，不派生替代 child 冒充原任务。

任何必要节点的 source、route、artifact、workspace 或 writer 无法确认，整树报告受阻，不先启动其他节点制造部分续跑。这个初版取舍避免父 Agent 看到假的完整恢复；后续如需部分树恢复，应另定义用户可见语义。

运行配置复用现有 frozen route/source 校验，重新附着当前凭据；不能持久化 token 或静默替换模型/effort。恢复未派发操作重新授权并取得新 permit；pending 权限/用户问题按原操作与版本重绑。结果未知的外部 preflight Hook 不重跑。

### 6. Preserve inputs, Goal, Workflow and recovery resources

挂起绕过 `reject_queued_inputs_for_shutdown` 的业务 dismissal，保留已接纳 input 与 consumed/reserved 事实。断连时释放 transient edit hold，但不启动消费。未提交草稿保持非权威；此 change 不额外承诺草稿在进程退出后的持久保存。

Goal 保留 identity、revision、累计用量及业务状态，冻结运行时间计算，恢复仍受精确预算/incomplete usage 规则约束。其他已明确的用户停止决定优先，挂起不得撤销。

Workflow 保留同一 Run/source/journal，增加非永久终止的 suspended execution disposition，满足旧执行结算 ACK 后才能进入下一 epoch 的规则。恢复可重新执行 Rhai 的确定性控制流并复用 journal；已关联 child 的 pending operation 必须 rebind，不能再次派发。这个既有 journal 重建不扩展为任意工具重放。业务 Paused/AwaitUser/BudgetLimited 的 Run 保持原状态。

未结束挂起引用是自动 GC/TTL/worktree 清理的保护根。显式删除、rewind、fork/import 沿现有所有权检查：副本不继承执行资格，不能重写仍被引用的恢复来源。完成/明确放弃后才按原资源规则解除相应保护，不在此 change 建立跨库删除事务。

### 7. Exit entry points and UI

执行会话的全局 Ctrl+C 一次请求挂起并退出；局部复制等已消费按键、只读 replay 和无会话 Welcome 沿既有语义。普通执行界面的草稿存在不应先触发 CancelTurn；挂起入口与显式停止任务分开显示。连续 Ctrl+C 只合并请求，错误界面用明确的“强制退出（可能无法完整恢复）”动作表达紧急退出。

正常 Quit 与首次可控 SIGINT/SIGTERM/SIGHUP 共用挂起入口。本进程退出会销毁多棵 root 时逐树持有操作并在同一个退出期限内等待；不影响另一 owner 的独立会话。第二次外部 OS 信号、panic 或用户明确强退仍可 best-effort 退出，不能伪装已保存。原有终端 keyboard fence 放在挂起和 writer 交接之后，保持独立有界 teardown。

状态显示区分节点 Suspended、操作 Interrupted/OutcomeUnknown、树 Suspending/Restoring/受阻和退出 drain；完成通知和累计用量以原 identity 去重，不用“自动发一句继续”驱动模型补猜执行状态。

## Risks / Trade-offs

- 快速停止与结算冲突 → 同一绝对期限，超时返回未确认；明确强退放弃完整保证，不伪造 ACK。
- 跨节点部分提交/恢复后再崩溃 → root 操作及节点 frontier 可核对，先登记再执行；旧挂起资格单次消费，已重新运行后的异常不回滚到旧 checkpoint。
- 外部副作用不能冻结 → 分开记录 Agent 挂起与操作未知，保留检查/人工处置入口。
- 当前取消和 Drop 逻辑覆盖面广 → 用不同 typed disposition 分流，验收 coordinator/guard 不产生 cancelled terminal。
- 已接纳 response、未完成工具协议与通用历史修复竞争 → 挂起分流先于 repair；继续保留配对，不凭空填成功结果。
- schema/运行环境变化 → 明确版本校验，精确错误并保留数据；不以降级为空会话换取表面恢复成功。

## Migration Plan

本次不修改产品实现或主规范。后续按 `tasks.md` 依次实施 typed schema/fold、owner 协议、子树恢复、外围集成和真实 PTY 验收，相关说明在同一实现 change 更新。

Timeline/control 的版本按仓库现有策略同步调整，不引入旧格式兼容分支；已挂起数据由能够读取其 schema 的版本恢复。不支持的版本只报告不可读取并保留来源，不能自动回退到普通中断修复。降级前需先用支持版本恢复/明确终止，或保存原目录供再次升级处理，不转换或覆盖原 checkpoint。

验收通过后才勾选实现任务、归档 delta 并全量/归档校验。当前仅文档格式通过不构成上述行为已可用。

# 对抗性审查（静态审查）

## 已核对的代码路径

- `tool_execution_error` 在 fatal turn-boundary 外层保留完整原始 ACP error 为 `cause`；顶层 detail 尽量从原 error data 提取。terminal ACK 再失败时，admission 路径将原 execution error 放进二次 terminal error 的 `cause`。
- `execute_tool_calls` 的错误现在映射为 fatal boundary error。`ToolLoop::Control(EndTurn/ResampleStep)`、权限拒绝/取消/超时、followup 和解析类结果仍作为正常 `Ok(ToolLoop)` 分支处理。
- fatal guard 位于 recovery loop 开头，在 Step boundary control 应用、completion recovery 和后续采样之前返回。因此合法控制结果不会命中该 guard；fatal 工具事件也不能先消费后到的控制或触发下一次完成检查。
- Timeline 的 `TurnEnded` 要求没有 active Step 和 open Tool。工具边界失败后，admission 仍尝试既有 terminal transaction；若底层 Timeline 因未完成事实拒绝 terminal append，外层错误保留原始 execution error，不会将未确认写入改报成功。

## 对抗路径与待验证项

- 故障 fixture 首版在模型输入 admission 前就失败，不能证明真实工具路径。该 fixture 已修正，当前等待重编译；以下结论均为静态审查，不能当作回归测试通过。
- 重编译后分别确认 Tool::Started、tool result publication、Tool::Completed ACK 三处注入故障都经过真实工具输入 admission，并检查原 error、单次 provider request、冷历史可重放性及无伪造 Completed/TurnEnded。
- 还需确认普通权限控制与 `ControlDisposition::EndTurn` / `ResampleStep` 仍由正常 ToolLoop 分支收尾；fatal early return 的静态分支检查不能替代这些运行回归。
- 变更将 `execute_tool_calls` 返回的所有 `Err` 都提升为 fatal。所查的普通拒绝、取消、超时、followup 和参数错误通常用 `ToolLoop` 表达；但故障 fixture 仅验证持久边界错误，不能证明每个内部错误来源的既有恢复行为。

## 真实故障暴露的后续修正

永久结果写入失败会在 buffered append 关闭 ChatState；下一条 ToolCompleted 只得到 AcknowledgementLost。原先仅封装 pipeline Err 的方案无法保留更早的存储文本。补充现有 PendingTimelineCommit.error_message，并在工具错误的 message 与 persistenceFailure 中附带失败序号/kind/message，pipeline 原错误继续存入 cause。独立审查核对了唯一生产构造、两个测试构造、finalization/teardown 消费者及 actor 串行失败顺序；字段只补诊断，不参与决策，不改变 retry/poison/deadline 或持久格式。成功仍清空 pending，最终永久失败覆盖先前临时错误。

工具 fixture 同时修正了真实 read_file 的 target_file 参数和 workspace runtime 绑定，防止仅测到错误参数的未派发结果。最终运行结果以 verification.md 为准。

## 验证闭环

前述待验证项已由 verification.md 的实跑结果完成：三故障注入通过，实际 dispatch 断言通过，相邻技能/控制/回合终态与原有采样恢复均通过。没有把测试 fixture 提前失败或 0 个测试算作成功。

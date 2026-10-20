# 退出挂起与重试边界审计

审计基线：工作区包含 `fix-suspended-background-command-receipt` 的 `Opaque → Ordinal(1)` 修正。本报告针对该修正之后的代码；不把原缺陷重复算作新发现。

## F1 · P1 · 已完成后台命令的通知正文冲突使退出持续失败

触发条件：普通后台 Bash 或 Monitor 已正常完成，完成通知已被 Timeline 接纳，命令仍在 terminal 的完成缓存内，然后请求退出。默认缓存保留 300 秒。

1. `tools/src/computer/local/terminal.rs:1690-1754` 的完成轮询发送带 durable ACK 的 TaskCompleted，成功后保留进程，直到 TTL 到期。`suspend_owned_commands` 在 `2176` 按 owner 枚举全部进程，包括已完成项；现有 `suspend_owned_commands_preserves_precompleted_result` 测试也要求返回这些证据。
2. `shell/src/tools/notification_bridge.rs:425-478` 使用 `format_bash_completion` / `format_monitor_completion` 的文本正文，以 TaskCompleted + Ordinal(1) 接纳。
3. `shell/src/session/actor/suspension.rs:991-1005` 对所有后台结算无条件再次使用 TaskCompleted + Ordinal(1)，正文却来自 `command_suspension_result` 的 JSON。它没有查询已接纳回执。
4. `chat-state/src/actor/mod.rs:481-508` 要求同身份重试的 payload_ref 相同。文本与 JSON 的 hash 不同，因此返回 InvalidNotification。该校验是正确的数据一致性保护。
5. `CommandsSettled` 先于通知提交写入。后续 retry 复用同一结算，仍构造冲突正文；即使命令缓存随后淘汰，冲突也不会消失。已消费通知仍保留 immutable receipt，是否已向模型交付不改变结论。

结果：没有 node checkpoint，退出持续未确认。这说明上一轮版本类型修正仍不足以覆盖普通“后台任务已经完成后退出”的路径。

修复边界：挂起应复用同 command 的既有终态回执；无回执时才接纳挂起结果。必须保留同身份异正文拒绝，不能通过改 UUID、增加版本号或放宽 Timeline 校验规避冲突。验证至少覆盖已接纳、已消费、首次挂起接纳、同 operation 重试。

## F2 · P1 · 超时重试把未回答的人工等待改成 OutcomeUnknown

`suspension.rs:843-848` 每次尝试重新采集 `pending_interactions` 和 attempt 列表。随后中止前台执行、关闭后台服务；等待 guard 退出会清理 registry。`PendingInteractionGuard::drop` 位于 `pending_interaction.rs:357-371`，只保留 Timeline 既有事实。

挂起在 `suspension.rs:1018` 克隆原 continuation，只在局部 cursor 上构造新 operations；checkpoint 成功前没有把这些等待信息保存成可重试的挂起证据。若服务/辅助 owner 到期后才停止，`1247-1254` 返回可重试错误。下一次尝试重新从空 registry 计算，并在 `1047` 清空 operations。

对已经有 ToolStarted、没有 tool_result、没有 command/child 结算的原人工等待，`1094-1128` 将其归类为 OutcomeUnknown；`1148-1169` 写入 tool_result 和 ToolCompleted。后续恢复因已有 tool_result 不再重新等待用户。模型调用的 ToolStarted 本来就早于权限预检，见 `tool/mod.rs:153-185`，因此“已开始的工具卡片”不能证明已经越过人工许可。

隔离测试已通过生产 `suspend_node` 复现：首轮返回 `suspension owners settled after the deadline`，次轮返回成功并提交 checkpoint；原调用被写成 OutcomeUnknown tool_result，checkpoint operations 也变成 `OutcomeUnknown / dependency_id: None`。夹具用挂起前台 Future 持有真实 PendingInteractionGuard 模拟等待所有权，未驱动完整权限管理器或用户交互。生产权限管理器会在 requester 消失后调用 `TimelinePermissionObserver::resolved`，同样会清理 registry；已经写入的 Suspended interaction 也未被当前重试分类器使用。

修复边界：同 operation 的重试应从稳定的挂起等待证据重建，或从 Timeline 的交互事实及已保存 continuation 核对等待，不能把进程内 Future/registry 释放当作业务终态。不能简单只扫描当前 open interaction：正常服务关闭也可能已经写入该 attempt 的 Suspended 终态。

## 已排除及范围限制

- 枚举其余生产 source/version 构造点，未发现另一个同类枚举配对错误：父子消息 Ordinal(2)，任务/子 Agent 完成 Ordinal(1)，PlanHandoff 使用 submission ID，WorkflowHandoff 使用 epoch/status/handoff，进度与仍在运行通知使用 Opaque。
- Pager 对用户显式 retry 生成新 exit_id；后端保留节点 operation_id，避免同一个 exit RPC 的缓存响应阻止新尝试。这条路径本身不是本次“r 无效”的原因。
- 原会话只有 CommandsSettled、没有 checkpoint 时拒绝冷恢复，符合规范的 fail-closed 边界；不能通过伪造 checkpoint 处理。
- Workflow terminal 写入失败会进入 Session fatal sink，不能仅凭 oneshot 被消费就断言正常重试必然 panic。本次不将该假设计为缺陷。
- MonitorProgress 每次投递生成 UUID，但未找到上游对同一事件进行 ACK 重投的路径；潜在重复不计为确认问题。
- Atlas Focus 未提供完整局部结构，报告依据直接源码、调用次序和测试，不声称覆盖整个仓库。
- 本轮未重放真实用户 session，未执行真实 provider/MCP 操作。完整文件系统故障、OS 调度和跨进程恢复仍超出这组局部复现。

## 测试缺口与处理顺序

现有测试分别证明通知接纳会拒绝异正文、terminal 挂起返回已完成命令、人工交互可以重新建立 attempt，以及命令结算可跨缓存淘汰重试。但它们没有组合“已完成通知 → 整体挂起”，也没有组合“等待 → 首次挂起超时 → 同 operation 重试”。上一轮只运行了结算复用和拒绝 payload 清理测试，因而不能证明这两条跨层场景正确。

先修 F1，恢复普通后台任务结束后的退出路径；再修 F2，保全等待关系。两项都需要行为 change 和跨阶段回归，不在这份只读审计中顺带改变实现。

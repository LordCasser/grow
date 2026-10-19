## Design

execute_tool_calls 的 Err 是执行管线失败，而工具正常返回的错误内容仍是 ToolLoop::Continue。将管线 Err 归入已有 fatal turn boundary，保留结构化 cause；completion recovery 在任何新 Step/控制边界前停止。由 emit_turn_ended 的既有事务先关闭未完成 Tool 为 outcome_unknown，再关闭 Step 与 Turn。存储已失败时不尝试伪造成功恢复；保存的有效前缀继续交由既有冷恢复规则检查。

Turn 错误 details 保存 fatal 诊断。若 terminal 持久化也失败，外层错误继续说明 terminal 未确认并携带原始 cause。不得用 catch-and-continue 或人工完成结果绕过 durable barrier。

## Adversarial validation

对真实采样返回的 read_file 工具调用注入 Tool::Started、工具结果发布及 Tool::Completed ACK 失败，检查最多一条 provider 请求、原始诊断、无成功假终态以及持久前缀可重放。检查 completion recovery 不启动；普通权限/工具结果路径继续由现有回归覆盖。

## Buffered result failure evidence

实际永久存储故障回归发现，普通工具结果通过 buffered append 发布；ChatState 在写失败后关闭，后续 ToolCompleted waiter 仅收到 AcknowledgementLost。现有 PendingTimelineCommit 已保留未确认事实及 error kind，补存同一失败的 message；这不是新日志或新状态 owner，不改变 exact retry、deadline 或持久化格式。工具错误封装同时携带 pipeline cause 与 pending persistence failure 的序号、kind/message，从而不丢失早于 ACK 丢失的原错误。

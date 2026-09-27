## Context

`PermissionManager` 按请求 mode 决定 Auto 分类或人工提示，子 Agent 默认 Auto 且不继承主会话当前 mode。Auto 的 `LlmPermissionClassifier` 把请求交给主 `SessionActor` 的 LocalSet 通道；worker 串行处理，并以出队时间建立 30 秒 deadline。Sideband setup 受 timeout 保护，但 begin/attempt/settlement 不全部受保护；provider 的首次 attempt 只获得剩余时间的一半。`SidebandRun::Drop` 已负责中断后的 fail-closed 结算。

## Decisions

1. 在发送分类通道请求时附带单调时间戳，worker 据此计算同一个 deadline。继续保留现有通道和 PermissionManager 单一状态所有者，不引入第二套授权来源。
2. 在 worker 最外层以偏置计时器约束整个裁决 future。Sideband 在已建立后若被取消，沿已有 Drop 修复闭合；结果发送仍受 oneshot 接收方存活约束。
3. 默认总期限设为 60 秒；首次 provider attempt 最多使用当时剩余时间的四分之三，第二次尝试使用剩余时间。早期结构化/传输失败立即进入重试，仍共享总期限。
4. 不改人工提示的独立 60/10 秒配置、主会话 timeout 结束 turn 和子 Agent timeout 仅失败工具的现有语义。独立 permission mode 继续由 child spawn 冻结。

## Risks / Trade-offs

- 较慢 provider 可能仍超过配置期限；此时 fail closed 是有意结果，操作员可调 `classify_timeout_ms`（上限 120 秒）。
- PermissionManager 本身串行等待分类或人工提示，其他请求与 mode 更新可能排队。这个较大的 actor 并发重构登记 backlog，不能为解决一个 Auto 期限故障而混入本 change。
- 最外层超时可在 Sideband 持久化等待期间触发；已有 Drop 修复负责终态，相关测试要证明无允许结果泄漏。

## Migration Plan

无持久化格式变化；配置未设置时使用新默认值。已有显式配置继续生效。

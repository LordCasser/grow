## Context

询问当前跨 coordinator runtime 缓存、source/target Timeline 与连接 incarnation。缓存不是 durable owner；Timeline receipt 和 source operation history 才能支撑冷恢复和重试。`inquiry_id` 仅在 source session 内稳定，因此 peer identity/incarnation 不能代替 source session 参与逻辑去重。

## Decisions

- 询问逻辑键固定为 `(source_session_id, inquiry_id)`，target session 由接收作用域隐含。peer identity 与 incarnation 继续用于认证、租约和审计归因。
- ID 只接受 runtime 生成的小写、连字符 UUIDv7 标准形式；拒绝其他文本别名，避免同一 UUID 被不同字符串键接纳为不同操作。
- target actor 先以 canonical Timeline receipt 核对同键操作：相同 payload 的终态可复用；已有 open receipt 不再执行模型询问。target 冷恢复时尚未终态的 open receipt 保持既有 `TargetRestarted` 终态，后续同键重试返回该终态，不重新采样。
- source 首次 `OutgoingStarted` 固定 target 与 question。重复启动只有 identity、target、question 均相同才是重试；已完成 outcome 复用。尚无终态的 source 查询报告 `SourceUnavailable`，不自动派发询问。
- request、progress、outcome 和 cancel response 均校验 inquiry ID 与当前操作身份。关联失败产生结构化错误，不把别的操作响应当作结果或触发重发。
- 预取消记录先于 dispatch 生效；已终态的远端询问取消返回 `accepted=false`。source/target scope 不匹配的取消不能作用于其他询问。

既有 runtime 权限校验、peer 握手、审计来源、Sideband 无工具执行和 foreground 隔离保持不变。

## Recovery and validation

验证围绕相同/冲突 payload、不同 source 同 ID、incarnation 变化、target runtime 丢失或 TTL 到期、source/target 冷恢复、迟到/错配响应和取消竞态展开。实现与回归完成前 tasks 保持未勾选。

## Why

Chat 流缺失 `finish_reason`、Responses 流缺失 terminal、Messages 流缺失 `message_stop` 都被归为不可重试的 Serialization。与此同时，非法工具参数和 doom-loop 已能绕过输出保护重新采样，但 attempt 的预览归属、各用量账本和恢复预算没有形成统一边界。

这次统一一次模型采样如何被接纳、丢弃和重试。仅扩充错误白名单会把同一问题继续分散到三个协议及不同调用方。

## What Changes

- **BREAKING**：错误表达区分流不完整、确定性协议违例、完整但无效的生成结果与本地失败；恢复决策组合错误事实、输出可撤销性、结算结果及剩余预算，不再由 `is_retryable` 或输出布尔值单独决定。
- **BREAKING**：复用请求/attempt 生命周期，贯穿 sampler、通知合并、Pager/headless、证据和用量归属。支持撤销的客户端按 attempt 展示；不可撤销的流在输出后禁止透明重新采样。
- 主/子 agent 模型步骤的每次真实 provider attempt 都向现有用量账本结算，成功接纳只更新上下文锚点，不重复收费。精确预算下未知用量继续关闭准入。
- 同一个尚未接纳的模型步骤共享恢复次数和期限；sampler 的同输入重试与 session 的凭据/上下文修复保留各自所有权，共用消费上限。
- 三协议保持各自完整性检查，统一输出可验证的失败事实。合法 length、拒绝、pause 等语义结果继续由 session 按已有语义处理。

已按用户授权完成实现、场景核对与相关回归；证据及保留限制见 verification.md。

## Capabilities

### New Capabilities

无。复用现有能力，不引入独立恢复服务、第二份会话历史或通用事务框架。

### Modified Capabilities

- `model-sampling`：统一 attempt 失败事实、恢复判定、逐次结算和共享恢复上限；将非法工具参数恢复纳入同一边界。
- `client-surfaces`：attempt 预览的开始、废弃和接纳，以及不可撤销输出的恢复限制。
- `session-timeline`：废弃 attempt 的证据与已接纳 Surface 分离；接纳确认不明时不得盲目重新采样。

## Impact

涉及 sampling-types 的错误语义，sampler 的流转换、request task、事件及 handle，shell 的采样/接纳/用量路径，通知合并和 Pager/headless 投影。沿用 Timeline、attempt evidence、Goal 准入/结算、TaskOutputTokenBudget；不新增数据库、依赖或后台服务。

非目标：保证供应商必然成功、自动切换供应商、无依据修补 JSON/终止标记、跨进程自动重放未确认请求，以及保证远端推理或远端工具 exactly-once。当前事故的真实线上尾帧仍待对应会话证据，设计不把代理故障当作已证实根因。

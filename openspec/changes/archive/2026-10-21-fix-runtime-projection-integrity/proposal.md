## Why

定向审计已证明三个缺陷：Model SkillLoad 元数据打断工具结果并导致成功结果被替换为取消；Responses 跨 Agent 消息生成没有 reasoning 的模拟调用；Chat Runtime assistant 绕过 reasoning_content 编码及启用判定。用户明确要求全部修复，先保护真实结果。

## What Changes

- 当前历史修复与 portable 投影对合法 Model SkillLoad 结果元数据进行同一批次归整，保留真实结果；不跨用户/assistant 边界。
- Responses AgentMessage 改为带明确来源/非人工授权声明的独立上下文，保留完整收件 JSON，不模拟模型调用。
- 已学习 Chat 路由为 Runtime synthetic assistant 编码空 reasoning_content，启用判定覆盖实际会输出的 Runtime assistant。
- 增加失败回归、跨协议/持久化/重复恢复边界回归，并进行对抗性审查。

## Capabilities

### Modified Capabilities
- `model-sampling`: Runtime 请求与 reasoning 编码、portable 工具边界。
- `session-timeline`: 完整性修复保全真实工具结果与元数据。

## Impact

sampling-types、chat-state 与对应回归；必要时核对 Shell 恢复测试。无新依赖、持久化格式或重试预算。不扩展未证实生产可达的 Native AgentMessage 插话状态机，不恢复已被旧修复删除的 Surface 内容，不修改原始 Timeline 事件。

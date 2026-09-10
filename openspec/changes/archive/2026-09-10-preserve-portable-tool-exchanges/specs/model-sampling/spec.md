## ADDED Requirements

### Requirement: Portable history preserves complete local tool exchanges

Portable 请求投影 SHALL 保留完整、无歧义的本地工具调用和匹配结果，并通过目标 backend 的结构化工具协议表达名称、合法 JSON 对象参数、关联 ID、结果正文及预算允许的图片。它 SHALL 移除旧 provider reasoning、签名、加密数据、输出 item identity/status 和模型诊断；SHALL NOT 将历史调用作为新的执行请求。原始 Timeline 和既有隔离事实保持不变。

#### Scenario: Tool attachments include eviction text
- **WHEN** 工具结果附件同时含图片与图片预算产生的文本，或只剩替换文本
- **THEN** 三协议在 live 和 portable 请求中都保留附件顺序及文本，不因附件位于 images 字段而丢弃非图片内容。

#### Scenario: Restore or switch provider
- **WHEN** 会话恢复或切换模型/backend 后中性历史包含完整工具往返
- **THEN** Chat Completions、Responses、Messages 请求分别保留配对的工具协议和内容，不包含被撤销的 native reasoning，切回原模型也不复活 native。

#### Scenario: Ambiguous or incomplete history
- **WHEN** portable 区域包含未配对、重复或无效工具记录
- **THEN** 不输出悬空调用、孤立结果或歧义配对，不伪造工具结果或修补非法 JSON；原始持久化证据不变。

#### Scenario: Valid same-route native continuation
- **WHEN** 请求仍有当前 epoch 的有效 native span
- **THEN** 该 span 继续使用完整原生内容，portable 处理不删掉其 thinking 或重复生成工具调用。

### Requirement: Portable boundaries keep tool exchanges together

Portable prefix 与 live suffix 的切点 SHALL NOT 将同一完整工具往返拆成被删除的调用和孤立结果。生成请求时 SHALL 在现有消息/native span 边界内闭合连续工具结果；wire 转换、请求 token 估算及投影证据 SHALL 使用一致的范围。

#### Scenario: Unsigned response precedes tool execution
- **WHEN** 完整 Messages 响应的 unsigned thinking 使 native 撤下，assistant 持久化后工具执行并追加结果
- **THEN** 下一请求保留该调用和结果各一次，不带无效 thinking/signature，已执行工具不重放。

#### Scenario: Several results straddle the prefix
- **WHEN** 同一批工具的多个结果分处 prefix 两侧
- **THEN** 投影保留完整配对及图片，估算与实际投影一致，不吞并后续 native span 或新消息。

#### Scenario: Normal final answer
- **WHEN** 修复后的请求收到合法 end_turn 且没有工具调用或其他待处理工作
- **THEN** 正常完成该 turn，不根据末尾标点或行动预告文本额外采样。

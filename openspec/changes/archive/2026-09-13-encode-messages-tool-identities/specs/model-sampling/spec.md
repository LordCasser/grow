## ADDED Requirements

### Requirement: Messages tool identity encoding preserves distinct exchanges
Messages 请求 SHALL 将中性工具调用 ID 与结果关联 ID 一致转换为 ASCII 字母、数字、下划线或连字符组成的非空有界 ID。请求中不同原始身份 SHALL NOT 因编码而碰撞。有效 native continuation 的原生内容和 ID SHALL 保持不变，生成 ID SHALL 避免与其冲突。编码 SHALL NOT 改写原始 Timeline 或执行身份。

#### Scenario: Portable IDs contain punctuation Unicode or excessive length
- **WHEN** 完整工具往返包含替换标点后会同名的 ID、Unicode 或超出本地编码预算的 ID
- **THEN** wire 保留每个独立调用及对应结果，ID 合法且一一配对，重复构造相同请求产生相同映射。

#### Scenario: Existing IDs overlap generated candidates
- **WHEN** 一个合法或 native ID 与另一个身份的初始编码候选相同
- **THEN** 保留已有身份，为被编码身份选择未占用 ID，并保持调用结果配对。

#### Scenario: Native tool use precedes a neutral result
- **WHEN** 同路由有效 native tool use 后续由中性历史提供结果
- **THEN** 结果引用原生 ID，thinking 签名及原生块保持原样。

证据：crates/codegen/sampling-types/src/conversation.rs 的 build_messages_request 与 portable/native tests。


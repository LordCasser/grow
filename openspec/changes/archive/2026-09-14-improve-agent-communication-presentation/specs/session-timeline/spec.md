## ADDED Requirements

### Requirement: Parent message receipts retain replayable presentation evidence

持久接收的父消息 SHALL 在所属会话保留期间保留可重建接收 UI 的来源、身份、投递模式和原始正文。正文的展示证据 SHALL 在消息消费后继续有效，UI 投影丢失不得删除收件事实。UI 恢复 SHALL 不重新接纳或消费消息，并保留现有模型输入的 agent guidance 来源语义。

#### Scenario: Consume then restore without UI cache
- **WHEN** 父消息已消费且会话的可丢失 UI 投影不可用，随后加载会话
- **THEN** 从持久收件事实恢复唯一的来源明确、正文完整的接收展示，同时该消息仍保持已消费。

#### Scenario: Cleanup and shared payloads
- **WHEN** 即时 payload 清理或启动 sweep 处理已消费通知，且其正文仍被父消息历史引用
- **THEN** 保留该正文引用及可读内容，包括与其他通知共享的内容；无引用 orphan 继续沿原规则清理。

#### Scenario: Durable commit and presentation publication are separated
- **WHEN** 持久接收完成而 UI 发布失败，或者同一父消息重试
- **THEN** 保留原接收身份和投递结果，后续恢复补全 UI；不写第二个接收事实或生成第二份模型输入。

#### Scenario: Historical body is unavailable
- **WHEN** 历史父消息正文缺失或校验失败
- **THEN** 展示保留可验证的收件身份并明确正文不可恢复，不伪造原文；未消费通知的模型上下文恢复仍执行原有严格校验。

#### Scenario: Retry changes delivery mode
- **WHEN** 同一父消息身份和正文被重试，但 interrupt 模式发生变化
- **THEN** 拒绝冲突请求，保留原接收事实与投递模式，不让 UI 或执行路径把重试参数当成已接收状态。

#### Scenario: Unsupported parent message representation
- **WHEN** 父消息使用不支持的 payload 表示版本
- **THEN** 明确拒绝该格式，不从自然语言包装猜测原始正文或静默改变模型输入。


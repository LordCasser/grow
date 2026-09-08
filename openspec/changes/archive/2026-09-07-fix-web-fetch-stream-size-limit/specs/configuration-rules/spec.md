# Delta

## ADDED Requirements

### Requirement: Web fetch bounds response accumulation
web_fetch SHALL 在接收解码后的响应正文时执行 max_content_length 检查，累计缓冲区不超过上限；超限 SHALL 返回 ResponseTooLarge，不等待响应结束。

#### Scenario: 持续分块响应超限
- **WHEN** 未结束的响应提供的解码正文已经超过 max_content_length
- **THEN** 立即停止累积并返回大小错误。

#### Scenario: 恰好上限
- **WHEN** 完整响应正文长度恰好等于 max_content_length
- **THEN** 正常处理正文；上限为零时允许空响应并拒绝非空响应。

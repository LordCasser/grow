## ADDED Requirements

### Requirement: HTTP hook decision bodies are bounded
阻塞型 HTTP Hook SHALL 在读取期间限制正文为 64 KiB；收到超出上限的分块时 SHALL 立即返回 Failed，不等待 EOF、不解析部分正文。超限 SHALL 由原有 hook 失败策略处理。

#### Scenario: 分块响应超过上限且不结束
- **WHEN** 端点发送超过 64 KiB 正文后保持连接未结束
- **THEN** Hook 返回响应超限失败，不等待请求超时或正文结束。

#### Scenario: 合法边界响应
- **WHEN** 正文为空或恰好 64 KiB
- **THEN** 正常读取并按既有决策解析处理。

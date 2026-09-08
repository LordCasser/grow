# Delta

## ADDED Requirements

### Requirement: Redirect targets require a new authorized call
web_fetch SHALL 不自动请求 HTTP 跳转目标，而返回 RedirectRequired、原始 URL 与解析后的目标 URL，要求新的工具调用经过正常授权；同主机也不例外。

#### Scenario: 同主机路径或端口变化
- **WHEN** 已授权 URL 返回指向不同路径或端口的 Location
- **THEN** 返回目标提示且不发送目标请求；目标被单独调用时经过普通验证与授权路径。

#### Scenario: 无效跳转目标
- **WHEN** Location 不可解析、包含凭据或使用不支持的 scheme
- **THEN** 返回跳转错误，不请求目标。

#### Scenario: 客户端展示
- **WHEN** 工具返回 RedirectRequired
- **THEN** 模型提示包含目标 URL 和新调用说明，ACP 将该次未取得内容的调用标记 Failed。

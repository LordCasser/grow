# local-coordination Specification Delta

## MODIFIED Requirements

### Requirement: Inquiry scoped operations
协调询问 SHALL 以 inquiry_id 与 target_session_id 标识询问和取消，并区分进度、结果、取消回执与错误。模型 `ask_session` 工具入口将缺失、null、空或仅含空白字符的 inquiry_id 视为未提供，由 backend 为新询问生成 UUIDv7；非空值原样按 UUIDv7 校验。外部协调协议/ACP 的显式 inquiry_id 和 `get_inquiry` SHALL 保持严格校验。

#### Scenario: Model tool creates an inquiry without a usable supplied ID
- **WHEN** 模型调用 `ask_session` 时 inquiry_id 被省略、为 null、空字符串或仅含空白字符
- **THEN** 工具将该值作为未提供 ID 处理，由 backend 为新询问生成 UUIDv7。

#### Scenario: Retry reuses a valid inquiry ID
- **WHEN** 模型以有效 UUIDv7 inquiry_id 重试相同询问
- **THEN** 工具原样传递该 ID，使 runtime 按同一询问身份处理重试。

#### Scenario: Invalid nonblank model ID is rejected
- **WHEN** 模型提供非空白但非 UUIDv7 的 inquiry_id，包括周围带空格的 UUID 字符串
- **THEN** 工具原样传递该值并由严格校验拒绝，不静默修剪或生成替代 ID。

#### Scenario: External protocol rejects an explicit blank ID
- **WHEN** 外部协调协议/ACP 调用方显式提交空 inquiry_id
- **THEN** 请求继续按严格 ID 校验拒绝，不触发新 ID 生成。

#### Scenario: 取消询问
- **WHEN** 客户端对已知 inquiry_id 发送 Cancel，并指定 target_session_id
- **THEN** 响应保留 inquiry_id 和 accepted 状态，不将取消回执伪装成询问答案。

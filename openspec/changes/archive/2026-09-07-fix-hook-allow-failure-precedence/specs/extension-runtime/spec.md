## ADDED Requirements

### Requirement: Hook allow decisions cannot hide execution failures
Prompt/Tool Hook 的 allow SHALL 仅在命令退出码 0 或 HTTP 2xx 时有效；其他执行失败 SHALL 交给现有 on_failure 策略。命令退出码 2 和显式 deny/block 仍保留拒绝优先语义。

#### Scenario: allow 后命令失败
- **WHEN** 命令输出 allow 后以非 0/2 退出，on_failure 为 block
- **THEN** 记录 Failed 并拒绝操作。

#### Scenario: 非成功 HTTP 允许正文
- **WHEN** HTTP 非 2xx 响应正文为 allow
- **THEN** 返回 Failed，不将正文作为成功允许。

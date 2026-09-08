## ADDED Requirements

### Requirement: CLI clipboard export reports actual delivery
CLI export --clipboard SHALL 消费剪贴板实际结果，失败时返回错误，不得无条件报告已复制。反馈 SHALL 保留目标后端及是否确认的语义。

#### Scenario: Clipboard backends fail
- **WHEN** 剪贴板结果为 Failed
- **THEN** 导出返回错误，不能打印成功信息并正常退出。

#### Scenario: Delivery is unverified
- **WHEN** 后端只能确认发送而不能确认到达
- **THEN** 保留未确认发送的反馈，不升级为已复制。

#### Scenario: Confirmed delivery
- **WHEN** 后端确认接收
- **THEN** 使用该后端反馈，并按共享统计规则显示文本量。

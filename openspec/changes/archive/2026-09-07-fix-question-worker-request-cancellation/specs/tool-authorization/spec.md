## ADDED Requirements

### Requirement: Question request cancellation preserves the worker
ask_user_question coordinator SHALL 在通知 Hook 阶段当前接收端关闭时结束该请求并继续服务后续问题，释放当前 pending guard；服务关闭与 Hook 失败仍遵从既有退出规则。

#### Scenario: 取消问题后再次提问
- **WHEN** 第一个问题在通知 Hook 等待前或等待中失去接收端，随后另一个有效问题到达
- **THEN** 后一个问题仍可经 ACP 返回响应，不因前一个请求取消而失去 coordinator。

## ADDED Requirements

### Requirement: Permission reset failure is visible without becoming model context

Shell 收到权限 Reset 通知后，若持久化失败 SHALL 发布 UI-only 错误提示，说明当前进程已撤销但重启后的权限文件状态未确认；该提示 SHALL 不进入 assistant/provider 对话内容。成功时不得发出失败提示。

#### Scenario: Reset notification encounters a write error
- **WHEN** 用户触发的权限 Reset 在根权限文件写入时失败
- **THEN** 客户端看到错误提示，Shell 不记录成功的 Reset 完成消息，模型上下文不包含此提示。

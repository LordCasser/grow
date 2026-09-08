## ADDED Requirements

### Requirement: Copy notices preserve delivery evidence
交互式 /copy 和 /export 的持久通知 SHALL 保留实际后端及确认程度，不得将未确认发送描述为已到达剪贴板。

#### Scenario: Unverified send with backup
- **WHEN** 复制结果为未确认发送且存在备份文件
- **THEN** 通知保留发送语义并显示备份位置，不宣称已复制到剪贴板。

#### Scenario: Confirmed backend and failures
- **WHEN** 后端已确认、仅文件成功或全部失败
- **THEN** 分别保留该后端反馈、文件回退位置或失败提示，成功备份位置仍可见。

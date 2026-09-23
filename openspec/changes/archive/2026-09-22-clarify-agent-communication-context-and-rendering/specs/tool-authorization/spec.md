## ADDED Requirements

### Requirement: Receipt-bound replies grant only communication authority

正式 agent 意见回复 SHALL 由 runtime-owned 收件证据授权。回复调用的权限投影 SHALL 不要求或授予文件/进程 RWX；初始 parent-to-child 消息仍保留既有 Write 投影。工具 eligibility、冻结参数和目标关系校验 SHALL 继续生效。

#### Scenario: Read-only child replies to received guidance
- **WHEN** read-only child 对实际收到的消息调用 send_subagent_message(reply_to)
- **THEN** 它可沿原参与方关系提交非 interrupt 意见，其文件/进程权限和父任务权限均不改变。

#### Scenario: Reply arguments cannot bypass routing
- **WHEN** 调用方同时提供 target 和 reply_to、请求 reply interrupt，或引用没有收件证据的消息
- **THEN** runtime 拒绝该精确调用，即使回复的 RWX 投影为空，也不能向第三方写入消息。

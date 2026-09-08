## ADDED Requirements

### Requirement: Command hook abnormal exits terminate process groups
命令 Hook 在进程组成功建立的情况下，取消或超时 SHALL 终止整个进程组，不依赖 session scope 是否存在。清理责任 SHALL 由执行 future 持有的守卫承担。

#### Scenario: 取消已派生孙进程的 Hook
- **WHEN** Hook 已派生同组后台进程后执行 future 被取消
- **THEN** 同组后台进程被终止，无 scope 时亦如此。

#### Scenario: 无会话 scope 超时
- **WHEN** 未提供 session scope 且 Hook 超时
- **THEN** 超时返回 TimedOut 并终止已建立的进程组。

# Delta

## ADDED Requirements

### Requirement: Recovery claims are owned before task polling
MCP stdio 重启与 HTTP 恢复 SHALL 在取得去重标记后立即将释放责任交给任务所有的守卫，包括任务首次运行前被丢弃的情况。

#### Scenario: 未运行任务被销毁
- **WHEN** 调度已取得恢复标记但 LocalSet 在任务首次 poll 前销毁
- **THEN** 标记被释放，不执行恢复动作或状态推送，同名标记可再次取得。

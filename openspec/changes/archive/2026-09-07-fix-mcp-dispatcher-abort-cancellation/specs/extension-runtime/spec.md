# Delta

## ADDED Requirements

### Requirement: Dispatcher exit cancels owned recoveries
MCP dispatcher SHALL 在正常退出和任务被强制终止时取消其启动的恢复任务。正常关闭保留等待恢复标记清理的流程。

#### Scenario: Dispatcher 被 abort
- **WHEN** 已调度恢复且 dispatcher 因 fatal 或关闭超时被 abort
- **THEN** 恢复 token 被取消，等待中的恢复不继续发起动作且释放去重标记；abort 本身不承诺同步等待全部任务。

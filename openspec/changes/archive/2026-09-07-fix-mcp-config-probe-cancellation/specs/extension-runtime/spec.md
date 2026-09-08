# Delta

## ADDED Requirements

### Requirement: Recovery configuration waits are cancellable
MCP stdio 和 HTTP 恢复 SHALL 在调度及重试循环的异步配置探测期间响应取消，不依赖探测先完成。

#### Scenario: 探测期间取消
- **WHEN** 恢复调度或循环等待配置探测且取消 token 被取消
- **THEN** 结束等待、不执行恢复动作、不推送 Disabled 或成功状态，已取得的恢复标记释放。

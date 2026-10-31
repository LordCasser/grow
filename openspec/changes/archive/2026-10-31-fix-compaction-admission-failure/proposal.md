## Why

继续审计无现场的“压缩完成后取消”反馈。provider 准入把 storage gate 的错误经 `is_ok()` 丢弃，并与前台控制停止折叠为同一个 `Cancelled`；因此摘要已经提交后，其他会话关闭共享存储域也会把当前回合错误地结算为取消。

## What Changes

- 用受控 HTTP 和持久化时序复现压缩完成后的存储准入拒绝，保留拒绝原因并以失败结算，禁止发出后续 provider 请求。
- 审计并测试异步 generation 在发布前与 replacement 后遇到取消信号时的提交边界，验证取消不会泄漏到后继 foreground。
- 记录内部 Cancelled 分支的真实触发来源及不能归因原反馈的限制。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `model-sampling`：后继 provider 准入保留存储失败，不能将其伪装成取消。

## Impact

Shell 的 provider 准入与压缩回归。沿用现有错误、存储 gate、取消 token 和终态持久化，不添加重试、备用 owner 或诊断框架。并发任务的其他修改保持原样。

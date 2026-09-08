## Why
问答 worker 在通知 Hook 阶段将单个 result receiver 关闭归入 hook_completed=false 并 break，导致后续问题全部失去 coordinator。

## What Changes
单次请求取消只 continue；shutdown、会话消失和 Hook 失败仍按原规则退出。

## Capabilities
### Modified Capabilities
- tool-authorization: 问答请求取消不终止服务。

## Impact
spawn 中 worker 提取为同模块启动函数以测试真实循环，无新状态；仅一处取消分支改行为。

## Why

2.4.0 的真实 PTY 验证发现，首条输入在 session 创建前排队，绑定后出队时，本地 checkpoint 会报告 `input intent is absent from the local draft snapshot`，ACP 请求未发出。普通历史恢复回归也复现，属于发布阻断。

现有 client-surfaces 契约要求旧草稿不得覆盖更新的未提交输入。本 change 修复其实现偏差，不绕过 checkpoint 或自动发送未保存内容。

## What Changes

- 在 cwd 到 session 的草稿交接中保留当前提交身份和精确快照。
- 补充真实 worker/绑定回归，并复验首轮发送和树挂起 PTY。
- 保留内容大小、图片、写入失败和迟到绑定的现有门槛。

## Capabilities

### Modified Capabilities
- `client-surfaces`: 明确绑定期间已出队输入的快照优先级。

## Impact

只修改 Pager 的本地草稿交接与测试；不改变 Shell 接纳协议、Tool 权限或执行资格。

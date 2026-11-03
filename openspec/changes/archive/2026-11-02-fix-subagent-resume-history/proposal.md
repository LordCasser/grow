## Why

主会话 resume 将 child transcript 延迟到首次打开加载。此前若收到 child 实时 Thinking、正文、工具或终态，显示层已有内容会阻止 deferred replay，导致任务 prompt 和已有执行记录不可见。

## What Changes

- 在 deferred child 首次接收实时正文或展示终态之前恢复历史，随后再应用实时更新。
- 保留首次打开的延迟加载及已重放 child 的幂等行为，不改执行、模型上下文或持久化协议。

## Capabilities

### Modified Capabilities
- `client-surfaces`：主会话 resume 后 child 历史与实时展示的衔接。

## Impact

Pager 的 child ACP/Grow 通知路由、首次打开回归和开发者排查说明。

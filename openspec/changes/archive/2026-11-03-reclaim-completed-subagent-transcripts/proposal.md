## Why

已完成 child 的 AgentView 仍保留完整 transcript、工具跟踪和渲染缓存，内存随累计任务数增长。现有持久历史和 deferred replay 已能承担重建，无需长期保留这些重状态。

## What Changes

- 卸载已完成、未选中且不处于 replay/reload 的 child 展示重状态；保留会话身份、控制、草稿及交互 owner。
- 重新打开或接收需展示的实时内容时，复用原历史回放；失败保持可见、可重试，不把失败当成已加载。
- 在 normal/minimal 共用事件循环执行回收，实际释放后复用 allocator release hook。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `client-surfaces`：child 展示的驻留/卸载及重新回放边界。

## Impact

涉及 Pager 的 child transcript、跟踪和展示缓存所有权，以及现有 child history stream 对后台任务展示事实的转发；不改变 canonical journal、runtime 控制或 child 收尾责任。保留既有工作区改动，与 coordinator 的容量修复分别验证。

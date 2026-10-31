## Why
挂起期间活动压缩必须先完成持久化和 Sideband/Goal 排空，但当前实现等待整个 foreground stopped；foreground 在 runtime handoff gate 关闭后 park，可能与挂起 owner 互等至 deadline。以真实 handle_prompt 和可控 Summary ACK 先复现。

## What Changes
- 验证活动压缩退出后挂起是否能收回前台任务。
- 若复现，等待压缩 lease 释放后终止并 join 已无压缩所有权的 foreground，而非等待 parked foreground 自行退出。
- 保留 ACK 未确认时的 owner、挂起身份与重试。

## Capabilities
### New Capabilities
无。
### Modified Capabilities
- `session-timeline`：挂起按压缩 owner 的边界排空后收回 foreground。

## Impact
Shell compaction lease、runtime suspension 与真实前台回归；无新超时策略或重试层。

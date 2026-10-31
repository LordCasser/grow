## Why

初次 Plan 审批选择要求修改后，Control 保存了反馈，但 Drafting 的模型上下文只包含原计划，工具结果与 handoff 通知也没有反馈正文。模型因此无法按用户的具体要求修订。现有 Amending 投影已包含反馈；本次补齐 Drafting 路径。

## What Changes

- 在 Drafting 的既有 PlanPhase 上下文中携带当前修订反馈。
- 验证连续修订、空反馈、重新提交和恢复后反馈与当前提交一致。
- 沿用原 Control、artifact、审批和 handoff，不增加状态或存储。

## Impact

- 规范：behavior-goal。
- 实现：shell session behavior 的 Plan 模型上下文及相关回归。

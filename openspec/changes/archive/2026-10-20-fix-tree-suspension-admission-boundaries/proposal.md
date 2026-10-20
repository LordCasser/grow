# 修复树级挂起准入边界

## Why
审计 F1/F2 已复现：冻结成功但 root 尚无持久 operation 时重试换身份；child 提前冻结导致人工等待在节点意图前 Suspended，被误写 OutcomeUnknown。

## What Changes
- 首次树冻结前持久确认 root SuspendRequested，所有重试复用原身份。
- 按最近 ExecutionReleased 隔离交互证据，保留当前执行周期内准入关闭后的 Suspended，包括节点意图之前的结算。
- 补初始化超时、冻结 ACK 丢失、三类人工等待的失败重试与冷恢复回归。

## Impact
影响 runtime_handoff、suspension 的证据折叠与测试。不新增事件、owner 或协调协议；不覆盖既有 coordinator 身份冲突保护。

## Why

Plan 修订返回后客户端立刻查询决定状态，查询可能早于后端 Control durable ACK，得到 unconfirmed。后端已进入修订但没有向客户端交付决定回执，窗口一直停留在 check confirmation。新的审批到达时旧 pending 字段仍然存在，又会把新窗口锁成确认中。

## What Changes

- Control 接纳决定后，通过既有 CurrentModeUpdate 交付该决定的原回执；客户端与状态查询共用精确核对和关闭逻辑。
- 新审批替换旧提交时解除旧决定的 UI 占用，保留原版本的未发送批注；迟到确认不能关闭新审批。
- 不新增轮询、重试计时器、决定账本或审批 owner。

## Impact

Shell Plan 审批交接、Pager 的决定确认与新审批接收；client-surfaces 契约。

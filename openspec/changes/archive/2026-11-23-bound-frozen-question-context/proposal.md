## Why

Session 01a2028b-1dd8-7693-ba23-52d67ffebb2f 的三次子代理问询都在首次采样前因父侧长历史超窗失败。Sideband 创建成功后，调用方报告 Failed，账本通过 Drop 记为 Cancelled。正常子任务终态交付已经成功。用户侧问也使用相同的全历史组装方式。

## What Changes

在接收方冻结快照中为原问题与系统消息预留空间，按时间近远保留可容纳的历史和完整工具批次。问询和侧问共用选择、来源记录与预算拒绝语义。明确被禁止的主动发消息与最终答复的适用场景。

## Impact

涉及 local-coordination 和 model-sampling。沿用既有 Sideband 预算、图片恢复、结果凭据和父子终态链。

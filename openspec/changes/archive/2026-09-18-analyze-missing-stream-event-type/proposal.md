## Why

用户要求分析会话 `01a0b3af-3159-73c0-9ae8-f1220fa9d6c7` 的 `missing field type`，判断容错及 retry 是否应调整。需要把上游原始错误、Grow 解析失败与重试决策分开，避免把内容拒绝误判为瞬态网络故障。

## What Changes

- 只读核对该会话及相关源码，记录脱敏证据和根因。
- 使用 loopback 协议探针复现现有解析和分类，给出最小修复建议与验收边界。
- 本 change 仅为分析记录，不改运行时代码或行为契约；`skip_specs: true`，不编造 delta。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

无。

## Impact

仅本 change 的分析、探针和验证记录，以及 backlog 中待独立处理的修复项。既有未提交实现保持；不向真实 provider 重发，不修改用户会话。

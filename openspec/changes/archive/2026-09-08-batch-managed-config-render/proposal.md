## Why
measure-managed-plan-batches 在1 MiB文件上实测release规划64条约84.5ms，单条约2.8ms。现有plan与render执行2n+2次全文解析，批量操作不必要地重复扫描相同内容。

## What Changes
共享原文解析结果计算条目状态与范围，一次生成所有替换和新增条目，再执行最终解析校验。保持公开接口和输出语义，因此作为纯性能重构skip_specs，不修改行为契约。

## Impact
config managed_text 内部渲染及回归。R17待确认字段继续保留。

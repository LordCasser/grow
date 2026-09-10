## Why

核对 session `01a08906-7afd-70e2-af4e-5ff9ea4db84c` 的冒号停顿是否已被当前 2.1.6 源码覆盖。需要区分原始响应结束、Goal 控制、压缩续接和请求投影中的工具配对问题。

## What Changes

- 保存限定范围的事故证据、版本比较和本地投影复现。
- 将确认的未修复缺口登记 backlog，后续单独处理。
- 本次仅审计及诊断材料，`skip_specs: true`；不修改产品行为、不创建 delta。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

无。核对 model-sampling、session-timeline、context-compaction、behavior-goal 现有契约。

## Impact

仅本 change 的审计材料与 backlog；不改 session、供应商配置、运行逻辑或已安装 Grow。

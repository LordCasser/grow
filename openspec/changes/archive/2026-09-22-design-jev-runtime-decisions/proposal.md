## Why

用户希望结合「Jev 原理与计算节省」「并行编程提示词 design」两段讨论，为 Grow 的模型与 effort 选择、memory、上下文压缩和授权形成可实施方案。需要先确定哪些 LLM 工作能够被实际移除，哪些现有确定性路径应继续保留，以及 Jev 如何进入已有运行时而不增加第二套状态权威。

## What Changes

- 核对相关主规范、当前实现和既有 change，记录四条接入链路及适用边界。
- 给出 Jev typed decision 接口、逐域策略、异常回退、授权证据边界、评测方法和后续 change 拆分。
- 本次只交付设计文档，不修改运行时、配置接口、持久化格式或已归档契约，因此设置 `skip_specs: true`。文中的未来行为均为提议，不是已经实现的能力。

## Capabilities

### New Capabilities

无。本次没有新增产品契约。

### Modified Capabilities

无。后续行为实现应分别建立 change，补充 WHEN/THEN delta specs，再实施和归档。

## Impact

仅新增本 change 的设计与验证记录。实现落点预计涉及 shell 的子任务解析、memory/compaction 编排、permission classifier 连接，以及既有 Sideband 的 typed request 记录；本次不改这些代码。

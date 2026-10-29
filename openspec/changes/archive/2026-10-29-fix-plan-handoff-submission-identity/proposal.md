## Why

Shell 已按 behavior-goal 的提交身份契约，将 Plan handoff source version 设为 session 单调 submission ID；Timeline 的旧校验却仍要求它等于 artifact revision。文档修订号与提交编号独立增长，退出后重新进入 Plan、相同正文再次提交等正常路径会出现两者不同。合法 receipt 因此被拒绝，live handoff 入口再将 admission 错误映射成截图中的 persistence acknowledgement lost。

## What Changes

- 将 Plan receipt 的 ordinal version 校验对齐独立的正 submission ID；artifact hash/revision/kind 仍通过原 source identity 和 owner 校验。
- 覆盖同正文跨 Plan 周期的真实 coordinator → Shell admission → Timeline consumption 路径，以及冷 replay、零值和非法 artifact 身份。
- 保留事件形状、审批匹配、单次消费与既有错误边界，不增加查询、账本、重试或新的交接框架。

## Capabilities

### Modified Capabilities

- `behavior-goal`：Plan handoff receipt 与已归档的 submission 身份语义一致。

## Impact

生产改动只涉及 chat-state 的 notification source version 校验。更新 Timeline/Shell 回归与开发指南；未复用原始用户会话或修改运行中会话。独立发现的已消费 receipt 二次检查问题只登记 backlog。

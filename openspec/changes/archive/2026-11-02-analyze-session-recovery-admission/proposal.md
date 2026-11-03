## Why

用户要求在 fork 多 session 旅程审计与本机恢复故障证据上继续分析：给出具体修复方案，区分必要恢复约束与可移除、降级的严格校验，提升正常编辑项目规则和重试恢复的可用性。

## What Changes

- 拆解 runtime_source_hash、规则/权限来源、挂起 barrier、resident load 与页面所有权。
- 记录分阶段的最小实现落点、反例和验证场景，并与先前 A1–A8 问题衔接。
- 只形成分析与后续实施建议；不操作用户 session、不修改产品代码。

## Contract

`skip_specs: true`：本 change 不改变现行契约，只保存方案评估。文中的目标行为不是已实现保证。实施涉及行为/状态/持久化时，必须另建对应 change，先写 delta specs 与 WHEN/THEN，再开发。现行严格契约可修订，不能把契约本身当成保留不合理限制的理由。

## Impact

Shell runtime source、actor spawn/restore、Agent rebuild、权限 classifier、load 生命周期，以及 Pager fork/close/delete/quit 旅程。已有恢复成功的新历史不可回滚或覆盖；fork 尚未实跑恢复成功，不能误报。

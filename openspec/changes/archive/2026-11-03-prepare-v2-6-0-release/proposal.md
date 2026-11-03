## Why

v2.5.5 后的候选工作树已包含会话交互、子 Agent 观察、恢复和辅助请求修复，需要整体审查并为用户确认的 v2.6.0 准备一致的 prompt、文档、版本与发布说明。

## What Changes

- 核对候选 diff、归档契约、调用方和测试，记录可复现问题及覆盖边界。
- 补齐遗留的 tool meta schema 快照；闭合词汇版本遗漏由独立带 delta 的 `version-subagent-tool-metadata` change 处理，不混入本非行为 change。
- 对齐现有工具语义的提示、用户指南和开发者说明；不引入新产品行为。
- 将 workspace/Cargo.lock 更新为 2.6.0，编写用户可见 release notes，执行适用发布前验证。
- 本 change 只做审查、文档和版本维护，skip_specs 为 true；若发现必须修改执行行为的问题，先另建带 delta 的 change。

## Impact

不改已归档行为契约。不自动提交现有用户改动、打 tag、推送或发布；这些动作不属于“准备 release”的交付范围。工作区既有修改全部保留。

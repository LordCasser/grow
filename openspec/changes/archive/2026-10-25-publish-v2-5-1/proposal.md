# 发布 Grow 2.5.1

## Why

用户要求整理当前改动并发布新版本。五项已归档变更需要提交、更新版本与发行说明，并通过现有官方发布流程交付。

## What Changes

纳入 Goal 完成报告、冷恢复上下文压力、技能 headroom、忙碌 effort 投影和全屏 /btw 弹窗改动；将 workspace 及本地 lock 包版本更新为 2.5.1。创建 annotated v2.5.1 标签，从 main 启动全部平台组及 publish=true 的官方发布工作流，验证同提交门禁、资产清单与本机公开包。

## Boundary

本 change 仅维护版本、发布说明和验证记录，不新增产品契约，因此 skip_specs。行为以五项既有归档及主规范为准；未完成的 MCP 多轮 elicitation 提案保持原状态，不宣称已实现。发布失败保留证据，不跳过门禁或移动已公开标签。

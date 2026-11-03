# 修复子 Agent 观察盲区

## Why

审计已确认多个公共入口、等待投影与取消分支仍会遮住可信结果或产生不真实状态，见 [审计](../2026-11-03-audit-subagent-observation-blindspots/analysis.md)。用户已授权修复。本 change 顺序完成已确认问题，不引入状态服务、第二份账本或新 watchdog。

## What Changes

范围：模型/ACP 历史查询、Pager prompt_status 必要事实读取、公共调用总期限、真实等待与终态计数、中断身份保留、采样取消、active 列表的缺进度表达，以及主模型在遗忘任务ID后通过现有工具按需重新发现活跃child。保留来源权限、Workflow 隐藏边界、completion receipt 和 worker 实际生命周期。未验证的资源饱和及平台问题仍独立留在 backlog。

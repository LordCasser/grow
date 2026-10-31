## Why

审计确认父 Agent 的委派拒绝被错误报告为全局 toggle 禁用，未知短名也被掩盖；Agents 面板修改 toggle 后，执行端和工具目录仍持有启动值。用户要求修复两项并补回归，同时授权个人 vulnerability-analyst 调用 general-purpose 和自身。

## What Changes

- 保留未知类型、全局禁用、父策略拒绝与校验不可用的真实原因，预检与派发一致。
- 通过现有 watcher/internal reload/actor 控制入口热更新全局 toggle，已有主/子会话刷新目录，后续派发读取当前策略。
- 保留已启动任务、父委派限制、深度上限与 Workflow 已冻结路由，不扩展其他 subagents 配置的热更新。
- 个人定义只增两项完整 ID，不引入短名别名。

## Impact

涉及 tool-authorization、configuration-rules；实现范围为 Task 校验、配置更新、会话 Agent 重建及对应回归。现有规范没有准确拒绝归因与 toggle 热更新契约，本次新增。

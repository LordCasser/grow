## Why

复审发现同一定义别名绕过委派限制、挂起恢复扩大出生 RWX、插件 toggle 目录遗漏及 CLI 定义目录缺失。用户要求修复四项，并对权限恢复完成真实挂起恢复端到端回归。

## What Changes

- 解析到正式 selector 后统一执行 toggle 和父 allow/deny；拒绝非规范路径 selector。
- 原 child 恢复读取原 Spawn 的有效出生 capability，与当前父上限取交集，不从当前 profile 默认重新授予。
- 所有普通目录合并 native/plugin/CLI 来源后过滤，保持解析优先级，CLI 来源进入会话重建；Workflow 仍消费冻结路由。
- 增加身份、目录、热更新及原树挂起恢复回归。

## Impact

涉及 agent discovery/builder、shell 委派解析/会话重建/恢复及 tool-authorization、session-timeline 契约。

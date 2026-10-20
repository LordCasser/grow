## Why
Settings 只有主 Agent 的默认权限入口，无法选择已存在的 `[subagents].permission_mode`。后端支持 Ask、Auto、Always approve，缺少设置闭环。

## What Changes
- 在 Agent 类别增加独立 Subagent permission mode，默认 Auto，支持三种已有模式。
- 保存到既有 `[subagents].permission_mode`，复用顺序保存与失败回滚；重启后应用，活动会话的权限保持既有生命周期。
- 补菜单、选择/重置、失败/连续保存及磁盘写入回归。

## Capabilities
### Modified Capabilities
- `client-surfaces`: 子 Agent 权限模式的 Settings 入口。
- `configuration-rules`: 定向持久化子 Agent 权限配置。

## Impact
Pager Settings catalog/snapshot/action/effect 与 Shell 配置写入。复用现有 RequestPermissionMode，不新增权限模式或授权机制。

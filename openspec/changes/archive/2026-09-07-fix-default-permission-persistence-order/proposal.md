## Why
默认权限设置仍使用独立 PersistPermissionMode，即使 session_id 固定 None。连续修改时旧失败无条件回滚，未受普通设置的保存顺序协调保护。

## What Changes
默认权限改用普通 PersistSetting(permission_mode)，复用现有按 key 顺序与基线协调；持久化分支只写默认配置，不通知当前会话。当前会话 NotifySessionPermissionMode 不变。

## Capabilities
### Modified Capabilities
- client-surfaces: 默认权限的连续保存与会话隔离。

## Impact
默认权限 setter、普通设置持久化分支和测试。旧独立保存协议暂保留，待用户确认删除，不顺带删功能。

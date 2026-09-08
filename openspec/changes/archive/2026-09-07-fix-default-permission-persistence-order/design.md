## Evidence
生产独立 PersistPermissionMode 仅由默认权限 setter 产生，session_id=None，WithRollback(previous)。无需 ACP 通知，可以直接使用已有普通设置队列。

## Decision
使用 permission_mode key 与 Enum canonical 值/回滚值。persist_setting 校验 canonical 值后通过 update_config 修改 ui.permission_mode。不新增队列或状态；当前会话切换仍走 NotifySessionPermissionMode，默认设置回滚只更新 default_permission_mode 和 current_ui。

## Verification
真实 dispatcher 连续 AlwaysApprove -> Auto，较早失败不得覆盖 Auto，后续失败回到 Ask；先成功后失败恢复 AlwaysApprove。每一步当前会话维持初始权限。验证默认设置和 reset 原有隔离测试。

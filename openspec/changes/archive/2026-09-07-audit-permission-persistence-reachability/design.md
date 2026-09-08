## Evidence
set_permission_mode 只产生 NotifySessionPermissionMode，修改当前会话；set_default_permission_mode 只产生 PersistPermissionMode { session_id:None, WithRollback }。全仓 BestEffort 的构造仅在 effects/tests.rs，生产只有匹配分支。SettingPersistFailedBestEffort 仅由该策略结果函数构造。

## Decision
R8 仅涵盖 BestEffort 变体、专用条件分支和结果处理，不把仍使用的 WithRollback 或 NotifySessionPermissionMode 一并删除。helper 接收 Some(session_id) 的通知能力目前只有测试传入，但不扩大删除候选范围，以便后续整理整个默认权限保存接口。

## Follow-up
默认权限仍未接入普通设置保存顺序协调。需独立复现并决定是否统一成普通 PersistSetting；本次可达性审计不宣称该路径已解决连续保存。

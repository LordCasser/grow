## Context
`SubagentsConfig.permission_mode` 默认 Auto，在 primary 启动配置解析时进入运行时，后代请求沿独立权限路线处理。Settings 当前只维护 `[ui].permission_mode`，不能复用该 key 代替子 Agent 配置。

## Decisions
- 使用独立 registry key `subagents.permission_mode`，Agent 分类、Shell owner、restart_required；沿现有 modal、typed Action、PersistSetting 队列与 rollback 处理。
- Pager 单独保留该配置的 typed 快照，启动时从有效配置读取；子 Agent Auto 不受主 Agent picker 的可见性 gate 影响，继续遵循既有后端 fail-closed 规则。
- Shell 使用既有配置写锁和原子写工具，只修改原始 TOML 的 `[subagents].permission_mode`，不序列化整个 SubagentsConfig 的运行时默认值。保留未知键、环境引用和主 Agent 权限，损坏源或段类型错误拒绝覆盖。
- 配置保存不即时更改已创建 session/child 的请求权限；提示重启后生效。

## Verification
菜单三模式/默认值/搜索/Minimal 可见；选择、reset 与无效值；连续保存失败恢复确认基线且不发送会话权限通知；真实 TOML 保存/重载、未知字段和错误源保全。仅执行相关回归，完成后清理本次 Rust 编译残留。

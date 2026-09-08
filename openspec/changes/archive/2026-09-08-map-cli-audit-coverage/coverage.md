# CLI 审查范围快照

2026-09-08，main 工作树。19 个顶层 Command 变体均找到实际分发；机器可读清单及源码 SHA-256 见 inventory.json。**19/19 是入口映射覆盖，不是功能审查完成率。** 尚未建立包含所有嵌套命令、flag、平台和失败窗口的完整分母，因此不发布总完成百分比。

| 入口 | 实际处理位置 | 下一步需核对的边界 |
| --- | --- | --- |
| Agent | cli::run_agent_command | headless 输入、取消、退出码及与 TUI flag 隔离 |
| Inspect | shell::inspect::inspect | 无效配置、项目发现及敏感值输出 |
| Doctor | cli::dispatch_doctor_if_requested → pager::doctor_cmd | 报告与修复隔离、平台探测失败 |
| Du | cli::dispatch_du_if_requested → pager::du_cmd | 符号链接、权限错误和目录扫描预算 |
| Leader | cli::run_leader_mgmt | PID 身份、过期记录与 kill 目标 |
| Mcp | pager::mcp_cmd | add/remove/doctor 子命令分别追踪 |
| Plugin | pager::plugin_cmd | marketplace 嵌套命令、更新失败和作用域 |
| Memory | pager::memory_cmd | 子命令和数据写入/查询失败 |
| Models | pager::models::list_available_models | 无效配置、无凭据、隐藏模型展示 |
| Sessions | pager::sessions_cmd | list/search/delete；不可把后端修复等同于 CLI 全覆盖 |
| Wrap | pager::wrap_cmd | 早分发、PTY/OSC52、平台限制与退出清理 |
| Export | pager::export_cmd | CLI 输出目标、历史读取失败及同步 IO |
| Trace | pager::trace_cmd | 导出前无用模型配置依赖、归档读取/输出边界 |
| Trajectory | pager::trajectory_cmd | 隐式 session 选择、监听范围与服务器生命周期 |
| Update | cli::run_update_command | check/install/channel 分支分别核对 |
| Version | cli::dispatch_version_if_requested / write_version | 早退出、JSON 和输出失败 |
| Completions | pager::completions_cmd | 早分发、各 shell 生成结果 |
| Worktree | pager::worktree_cmd | 嵌套 db 操作、并发 Git 状态与删除范围 |
| Dashboard | cli::flag_dashboard_at_startup_if_requested | 配置/env 禁用错误、进入 TUI 后生命周期 |

上表记录后续审查边界，不声称此前从未审过这些模块。历史记录必须逐项阅读 verification 后才能抵扣具体场景，不能以标题关键词算完成。

## 本轮核对的既有证据

- audit-doctor-execution-boundary 的 verification：源码追踪，未执行 doctor；指出 TUI 同步探测，不能当作完整 doctor 运行验证。
- fix-atomic-transcript-export 的 verification：4 项 export 测试，含写入失败、权限与符号链接；明确未测试 Windows、掉电、父目录 fsync。只覆盖该提交边界。

## 隐藏与禁用不等于废弃

- Wrap 在非 Unix/Windows 平台隐藏；这是平台条件，不是未接线。
- Dashboard 受 dashboard_enabled 检查，禁用时在 TUI 启动前报错；不是死分支。
- CLI 中还存在隐藏的 enterprise 更新选项、trust/no-ask-user/background/client-identifier/hunk-tracker/terminal/fs-read/fs-write/no-auto-update/installer/log-sampling/leader 等参数。这些不包含在 19 个子命令分母，需按各自消费者逐项审计。
- 嵌套 enum 包括 LeaderMgmtCommand、DoctorCommand、McpCommand、PluginCommand、MarketplaceCommand、MemoryCommand、SessionsCommand、WorktreeCommand、WorktreeDbCommand。未因顶层分发存在就标记它们完成。
- 无子命令 TUI、slash 命令注册表、工具注册与禁用工具、服务端协议、Provider 路由和平台实现不在这个局部清单内。

## 下一调查

cli main 的 Trace 分支加载 effective config 并构造 AgentConfig，但 pager::trace_cmd::run 只接收 TraceArgs。需用隔离配置故障验证导出是否被无关配置阻断，再立行为 change；本轮没有执行真实导出或删除会话。

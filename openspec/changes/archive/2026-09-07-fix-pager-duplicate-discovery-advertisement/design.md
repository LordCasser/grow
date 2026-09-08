## Evidence
见已归档 audit-discovery-reload-fanout。重新核对 session_setup.rs::reload_skills_from_disk 两条分支：无 pending 时直接 send_available_commands_update，有 pending 时 take_pending 产生 send_available_commands=true，经 apply_skill_update_effects 发布。named_workflow_snapshot 每次发布时读取当前 workflow。

## Decision
移除 Pager Skills 分支额外 advertise_commands_all_sessions 调用，保留 refresh_new_discovery_dirs 调用但不使用其返回值。不把“一次事件只由重读路径发布”扩大为跨所有事件的全局 exactly-once 保证。读取失败、运行任务并发、重复 OS 事件的语义不在本次改变。

## Validation
这是调用删除与未使用绑定移除，无新增逻辑。通过定向 diff、Rust 格式/语法检查及 OpenSpec 验证核对。磁盘不足，不声称执行了 Pager Cargo 构建或端到端计数测试。

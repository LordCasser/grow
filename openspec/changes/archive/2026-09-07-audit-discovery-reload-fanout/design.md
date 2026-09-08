## Scope
从 config/watcher.rs 的 DiscoveryChange 到 Shell 与 Pager 消费者，再跟踪技能 baseline 与 workflow snapshot。已有目录替换修复独立留在 fix-discovery-watch-directory-replacement。

## Evidence
- shell/src/agent/app.rs：Skills 两个分支都发送 grow/internal/reload_skills；Workflows 发送 reload_workflows。
- shell/src/extensions/session_admin.rs：前者调用 reload_skills_all_sessions，后者 advertise_commands_all_sessions。
- shell/src/agent/mvp_agent/agent_ops.rs：分别向现存 session mailbox 发送 ReloadSkills / AdvertiseCommands。
- shell/src/session/actor/run_loop.rs：每会话项目 watcher 的 Skills 直接调用 reload_skills_from_disk，Workflow 直接发布命令。
- shell/src/session/actor/session_setup.rs：reload_skills_from_disk 重新列出技能，写 baseline，apply_pending 无变化时仍发布命令；有变化进入 apply_skill_update_effects。
- tools/src/types/skill_discovery_tracker/mod.rs::take_pending：生产构造 SkillUpdateEffects 时 send_available_commands 恒为 true，所以有变化也会发布命令。
- shell/src/session/actor/workflow_run.rs::named_workflow_snapshot：发布命令时重新 scan registry / catalog，没有只依赖旧技能数量的缓存短路。
- pager/src/acp/spawn.rs：Skills 分支 reload_skills_all_sessions 后，在新目录注册时又显式 advertise_commands_all_sessions；结合上述路径存在重复扫描/发布机会。

## Conclusion and limits
没有从当前控制流证实“根删除分类为 Skills 导致 workflow 入口永远不刷新”。这不等价于端到端 UI 测试通过，也不证明所有异常路径、事件丢失、异步重读并发都无问题。磁盘不足，未重新构建 Shell/Pager。

## Separate follow-up
Pager 新目录事件的重复发布属于可优化项，应单独验证消息数量再修复。ReloadSkills mailbox 分支会 spawn_local；多个重读可并发，需独立核对扫描快照与提交次序，当前未复现乱序覆盖，不能写成确认 bug。

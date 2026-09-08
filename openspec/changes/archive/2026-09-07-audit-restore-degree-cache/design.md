## Evidence
- pager/app/session/mod.rs 定义 AgentSession.restore_degree，默认 None，注释明确当前无渲染消费者。
- root/effects/helpers.rs 从两种 wire 结果解析 degree；effects/mod.rs 将其随 TaskResult 转发，task_result dispatcher 解构转给 load/fork handler。
- session/load.rs 与 session/fork.rs 仅将字段写入 agent.session.restore_degree。全仓 .restore_degree 的非测试 Pager 引用仅这两处赋值；测试检查保存/清空。
- 真实通知使用 (code_restored, restore_summary)，明确显示恢复成功或失败及摘要，没有读取缓存。
- workspace/session/git.rs 的 RestoreDegree/RestoreDecision 属于恢复结果构造；Shell 的 restore_code 与 worktree 响应适配仍输出它。不能把内部缓存无消费推断为共享类型或恢复操作无用。

## Candidate boundary
R12 仅会话缓存、默认值、赋值、专属缓存断言以及仅为填充缓存的内部转发参数。保持原协议解析/非法值校验、共享类型、Shell wire 数据、实际恢复逻辑及用户结果摘要。真正删除前再核对新消费者。

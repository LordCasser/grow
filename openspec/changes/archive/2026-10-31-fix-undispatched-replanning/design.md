## Decisions

`terminal` 表示真实 child 生命周期已经终止，`dispatch_failure` 表示执行未准入；两者均不能承担后续 child 确认责任。父项修改继续保留每个来源的 ReplanChange，但对明确未派发关联直接记录 Unapplied，原因来自原失败事实。

复用 `mark_link_undispatched` 清理已存在的未闭合 replan。同一失败的重复提交在状态已闭合时幂等；若历史状态存在后生的 pending，则闭合它们并提交新的 Planning 事实。SessionActor 在 hydrate_local_plan 投影前执行同样的转换，保留旧事实和失败身份，清洁恢复不产生额外提交。未知执行和仅 Released 的活跃执行保持原门槛。

完成检查在一次 process_conversation_turn 内保存上次提示的具体 blockers。首次提供处理机会；下一次 final 若 blockers 完全相同，返回带 `planning_completion_blocked` 的明确错误，禁止 Agent completion recovery 再次采样。清空或改变 blockers 可继续使用既有流程；显式等待、max-turns、取消优先沿现有边界处理。该状态只属于本轮，新的用户输入可以重新处理，不新增持久账本、计时器或配置项。

## Validation

- LocalPlan：先失败再修改父项；required/optional、Released/未 Released；活跃关联仍有 pending；重复失败修复 Pending/Dispatching 等遗留记录且保留失败审计。
- Actor：恢复含 dispatch_failure 的遗留计划，确认 durable Unapplied、Released 后完成及 scope 切换、重复恢复无新提交。
- 生产 sampler 回归：相同阻塞两次 final 后仅两次模型请求，普通 final 和 StructuredOutput 都停止，Goal 内部错误沿既有暂停路径结算；已解除义务可正常完成。
- 定向 Rust 测试、OpenSpec strict 全量与 archive 校验、变更 diff 检查。

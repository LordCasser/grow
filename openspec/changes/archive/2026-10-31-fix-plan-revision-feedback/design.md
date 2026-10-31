## Evidence

`accept_plan_decision` 将要求修改映射到 Revise，保存 feedback，并由 `commit_behavior_mutation_or_restore` 提交 PlanPhase 上下文。`plan_phase_model_context` 的 Amending 分支带上反馈，Drafting 分支遗漏。live 工具结果和 idle handoff 都只描述状态，不能弥补此遗漏。

## Decision

Drafting 与现有 Amending 一样，从 snapshot 的当前 handoff 读取反馈，附在原计划之后。复用已有空白处理，不复制到工具结果或通知，也不改变审批权限。新 artifact 的既有记录逻辑清除旧 handoff，后续提交不继承旧反馈。

## Verification

先以真实 BehaviorCoordinator 的提交和修订转换构造失败断言，再核对 Drafting/Amending、连续修订、空反馈、快照恢复和重提交。增加 SessionActor 的持久 PlanPhase 投影验证，确保不是只保存 feedback 字段。

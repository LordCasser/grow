# Upstream targeted review — 2026-10-11

## Scope and status

- Reviewed upstream target: `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`。
- Prior documented broad review endpoint: `bc7f02eddd3d84085849dc19ed216f11c23b0571`。
- 本次是对固定目标的 MCP 多轮、可保留计划体验两主题评审；不是对 bc7f02ed..2bdd1d6 全提交/全功能穷举审计。remote latest HEAD 未核验。
- Grow baseline: `7b7d56ab98a686223bc659567c79c1363729cd66`。研究和本次文档开始时均有109条原有工作树状态；HEAD 不变不能证明工作树等于 HEAD。
- 状态：两项已评审、已建立提案；本次没有实现、合并、cherry-pick、提交或推送产品代码。
- 本地上游辅助 checkout 为 `07e35a3dfeed2f200d319ef6c893b5ea286d9a51`，仅用于路径定位；关键结论复核自公开固定2bdd源码。

## Decisions

| 方向 | 决策 | 落点/限制 |
| --- | --- | --- |
| MCP Form/URL 与 MRTR | 已评审、已立项、未实现 | [add-mcp-multi-round-elicitation](../../add-mcp-multi-round-elicitation/proposal.md) |
| 保留、重新审阅、稍后批准执行计划 | 已评审、已立项、未实现 | [retain-plan-for-later-review](../2026-10-19-retain-plan-for-later-review/proposal.md) |
| 最顶部 task 布局 | 保留 | 两项实现均不迁移到统一 Dock |
| 输入区上方统一 Dock | 不采纳 | 不设实现任务 |
| Memory | 暂不跟进 | 不设实现任务 |
| 模型→上下文窗口→推理强度连贯选择 | 不采纳 | 不设实现任务 |
| Remote Control / Agent Host 跨设备 | 暂缓、未排期 | [backlog](../../../backlog.md)，不展开实现 |
| 其他 TUI/大型功能 | 不在本次范围 | 无全量审计或采纳声明 |

## Findings and provenance

1. **Grow HEAD 已有** Plan 四阶段、不可变 artifact、冻结 snapshot 审批以及 `resume_plan_approval` 在无 active turn 时接纳 PlanHandoff 并进入原仲裁。不能重复作为缺失能力立项。[behavior](https://github.com/LordCasser/grow/blob/7b7d56ab98a686223bc659567c79c1363729cd66/crates/codegen/shell/src/session/behavior.rs#L103)、[resume](https://github.com/LordCasser/grow/blob/7b7d56ab98a686223bc659567c79c1363729cd66/crates/codegen/shell/src/session/actor/tool/mod.rs#L962)。
2. **Grow HEAD 静态差异**：Pager 无审批 view 时回退 plan.md，而 Shell 使用 artifacts/plan/<hash>.md；需要 canonical 查询闭环，尚未 PTY 复现。[Pager](https://github.com/LordCasser/grow/blob/7b7d56ab98a686223bc659567c79c1363729cd66/crates/codegen/pager/src/app/agent_view/plan.rs#L77)。
3. **上游固定源码**：KeptPlan 与 after-turn review 可借鉴，但公开本地后端 post_turn_plan_review 默认 false；测试桩开启不证明完整执行后端可移植。[gate](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/src/app/agent_view/session.rs#L23)、[plan](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/src/app/agent_view/plan.rs)、[kept_plan](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/src/app/agent_view/kept_plan.rs)。
4. **Grow HEAD 归档证据**：2026-09-23 已撤销“等标准/SDK成熟”前提，当时缺产品用例。此次重新立项从正式 MRTR 出发，旧历史文档的 draft 门槛不再适用。[verification](../2026-09-23-reconcile-backlog-active-debt/verification.md)。
5. **上游演进**：[72a61251](https://github.com/xai-org/grok-build/commit/72a61251) 引入 MRTR；[75810042](https://github.com/xai-org/grok-build/commit/75810042) discovery 主要用于 HTTP/HTTP-auth/ACP，stdio 保留旧路径；[f0e3be11](https://github.com/xai-org/grok-build/commit/f0e3be11) 升 rmcp3.4；[97f190f6](https://github.com/xai-org/grok-build/commit/97f190f6) 增加 RequestTracker。最终行为以2bdd为准，不能丢掉后续短 startup budget 修正。
6. **上游固定源码限制**：ElicitationInbox 单槽替换、Pager 单 pending 槽不等于有界 FIFO；UI 交互 id 与原模型 tool_call_id 要分离。[tracker/inbox](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-mcp/src/elicitation.rs)、[bridge](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/session/mcp_elicitation.rs)。
7. **工作树增量，不能用 HEAD 链接证明**：输入重试身份/未知 ACK、runtime observation、子任务收尾/恢复资源和 storage admission。对应未提交归档目录为 [输入恢复](../2026-10-08-preserve-uncertain-input-and-recovery-resources/proposal.md)、[状态观察](../2026-10-08-observe-session-runtime-health/proposal.md)、[收尾](../2026-10-08-recover-subagent-terminal-commit/proposal.md)。这里只记录实施对齐依赖，不声称已发布。
8. **设计建议**：有界并发交互、保留候选投影、稳定决定身份及两个 acceptance 矩阵不是已有实现。MCP 使用原工具 owner，计划使用原 SessionActor/Behavior/Timeline owner；Rhai/Goal/队列职责保持独立。

## Protocol references

- [MCP 2026-07-28 MRTR](https://modelcontextprotocol.io/specification/2026-07-28/basic/patterns/mrtr)：同 logical operation、新 wire id、按 key 回答、opaque state。
- [Elicitation](https://modelcontextprotocol.io/specification/2026-07-28/client/elicitation)：Form/URL、accept/decline/cancel；URL 同意不是完成，秘密不能经 Form 收集。
- [上游最终 MCP loop](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-mcp/src/servers.rs#L1666)。

## Verification limits

没有重新运行产品/PTY 测试，没有真实第三方 Form/URL 互操作结果；研究中的历史测试记录不冒充本次结果。原报告已交付为独立研究，后续实施验收以两个 change 的矩阵为准。

上游跟踪字段表示“检查过这个固定目标的指定主题”，不表示已合并、已实现、已完成区间所有提交评审，也不修改 Git 远端引用。

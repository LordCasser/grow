# 独立设计审查

审查日期：2026-11-03。审查者为独立子 agent `injection_design_review`。本轮只审查提案，不审查尚不存在的实现。

## 范围与依据

已阅读 proposal/design/evidence/tasks 和三个 delta，并核对当前 Grow 的 inquiry audit、已有 outcome 复用、通知消费及最终回复边界，以及用户指定本地 DSH 的 `agent.ts::send/inject/cancel`、`inbox.ts::claim`。参考的 DSH checkout HEAD 为 `ce7073d51254c26cd63b22a60835b8f948deb447`。两个工作区均含前序未提交内容；没有修改业务代码，也没有执行 Rust/DSH 运行时测试。

## 初审发现

1. **[P1] 缺失收据重建必须验证原回答方，而不能从新收据反推所有权。** 初版 design §3 用收据确定回答 Session，§5 又允许从成功 audit 重建缺失收据。若历史被 fork/复制，原成功记录与本地缺失收据可能被误认为新的本地同步义务。当前 `IncomingInquiryAudit` 没有目标 Session 或实际 Sideband 字段，不能假定已有证据足够。必须在新 canonical 成功证据中绑定原 `target_session_id` 和 Sideband 所有权；重建时核验其与当前合法 writer 所属 Session 一致。来源历史可以展示，但不能把待消费执行权带入新 fork。
2. **[P2] “只同步新成功事实”需要可持久判定的资格。** 初版 §6 禁止历史回填，但 §5 对成功 audit 的恢复描述没有区分新旧。当前 audit 只有 source/question/outcome 等字段；不能以启动日期、UI 文案或事件位置推断是否需要新同步。新成功事实须具有明确的契约版本/typed proof，旧成功审计只保留历史；需分别测试“新证明存在但 Received 缺失”和“没有新证明的旧 Answered”。

上述问题已提交主 agent 并完成设计修订，最终复核结果见下节。它们是设计约束缺口，不是声称当前运行时存在已复现的注入错误。

## 其他检查

- 正确区分 DSH 非抢占 steer 与 Grow 当前 sampler request 软抢占，未要求复制四套公开 API。
- passive 来源不进入 `pending_interjections`，不触发 parent interrupt；当前通知仅 TaskStillRunning 禁止 autostart，因此新增来源必须显式禁止 idle wake。
- no-wake 不等于活跃 Turn 绝不增加 Step。设计规定 final-close cutoff，并保留 cutoff 后 pending，未承诺修改在途请求。
- Received 与 Consumed 的职责分开；Consumed 和精确输入同一 durable 事实，terminal→Received 缺口由已有 canonical 来源协调，不添加第二状态库。
- Sideband 生成失败/取消与成功同步分开；同步失败不回滚真实 Answered；外部调用方不向来源伪造工具结果。
- 合成输入不带 PermissionEvidence，不把咨询中的批准文字转成人类权限；保留 FIFO edit hold、Goal/Plan/Workflow 准入和用户 steer 旧目标失败语义。
- 实现、对抗测试和归档任务保留未完成。只读 observation/UI 回放不应取得 writer 或执行补齐动作；恢复补齐属于合法 writer 路径。

## 已执行验证

`openspec validate separate-execution-control-and-context-delivery --strict --no-interactive` 通过。它只证明 delta 结构与场景替换满足 OpenSpec 校验，不证明运行时正确性。全量校验由主 agent 汇总。

## 最终复核

已复核修订后的 design §3/§5、session-timeline delta 与 tasks：

- 新结构化成功证明绑定原始 target identity 与实际 Sideband，重建必须验证当前 writer Session 和 Sideband owner，不从 fork 的当前 actor 或缺失收据推断所有权。delta 增加了“继承成功证据但没有本地收据”的 fork 场景，任务 2.1/3.8 对应覆盖。
- 新契约的 typed proof 才构成同步义务，未携带该证明的历史 Answered 明确不回填；没有引入布尔 feature 开关或并行账本。
- 修订后重新执行本 change 的 OpenSpec strict validation，通过。

两项初审问题已在方案层关闭。未发现其余阻塞本次设计交付的可行动问题。本结论仅表示方案可进入有约束的实现阶段；不是实现批准流程，也不代表运行时、故障注入、跨 provider 或恢复测试已通过。实现前任务与后续独立代码审查仍保持未完成。

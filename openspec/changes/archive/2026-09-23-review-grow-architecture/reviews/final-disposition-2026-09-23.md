# 本次架构审查的收口边界

本 change 的三份报告分别固定 2026-09-16 model-sampling/attempt admission、2026-09-17 已合并响应恢复、2026-09-20 子 Agent 工具装配的源码基线。报告里的位置、风险和测试结果只代表各自基线，不能直接宣称 2026-09-23 当前树仍有同一缺陷。审查产出是具体后续 change 的入口，不是“全仓已审查”的证书。

- [model-sampling / attempt admission](model-sampling-attempt-admission.md) 所发现的 response-admission 不明 ACK 和 Timeline/cache 裂缝，分别由 [response-admission ACK 修复](../../2026-09-17-reconcile-response-admission-ack-loss/verification.md) 与 [response replay projection 修复](../../2026-09-23-reconcile-response-replay-projection/verification.md) 后续处理；前者与后者的验证范围以各自归档为准。
- [已合并响应恢复复核](response-recovery-merged-2026-09-17.md) 的 rewind、fork、resident 和 sync 故障窗口由上述 response replay projection change 独立实现和验证。旧报告保留原始反例，不再作为当前缺陷清单重复登记。
- [子 Agent 工具装配审查](subagent-tool-assembly-2026-09-20.md) 的审核身份和执行证据缺口由 [fix-subagent-reviewed-tools](../../2026-09-20-fix-subagent-reviewed-tools/verification.md) 修复。目录 typed reason、跨进程文件冲突保护及 native 后代用途语义仍在 [backlog](../../../../backlog.md) 单独跟踪。

原 `tasks.md` 的“当前全仓架构总图、三个旧切片重新审查、逐项覆盖全部行为特性、全仓综合验证”在 2026-09-23 被显式撤出本 change，而非标记为已完成。没有固定源码版本、能力边界和可运行验收矩阵时，持续全仓审查会让报告在完成前失效。后续以一个具体故障或风险边界为单位，从当时的主规范和真实入口建立独立 OpenSpec change。

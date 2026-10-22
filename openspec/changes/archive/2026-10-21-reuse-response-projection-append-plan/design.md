# Design

先以 ignored、可重复的 benchmark 固定现状：构造 128、512、2048 个带合法 admission identity 的 response 与一一对应的 existing projections，计时当前 reconcile 加 load 中 `existing.iter().any(exact_match)` 等价路径。之后将 planner 的明确 canonical append 计划直接交给 load 调用方，避免 existing clone、完整 reconciled 输出及 P×U 重复扫描。

planner 继续负责 exact payload 冲突、anchor 与 candidate/suppression 决策。Timeline rewind、cursor 和 physical event identity 规则保持不变；不跨 Timeline revision 缓存 branch 分析。
